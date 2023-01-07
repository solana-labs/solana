use {
    crate::{
        block_error::BlockError, blockstore::Blockstore, blockstore_db::BlockstoreError,
        blockstore_meta::SlotMeta, leader_schedule_cache::LeaderScheduleCache,
        token_balances::collect_token_balances,
    },
    chrono_humanize::{Accuracy, HumanTime, Tense},
    crossbeam_channel::Sender,
    itertools::Itertools,
    log::*,
    rand::{seq::SliceRandom, thread_rng},
    rayon::{prelude::*, ThreadPool},
    solana_entry::entry::{
        self, create_ticks, Entry, EntrySlice, EntryType, EntryVerificationStatus, VerifyRecyclers,
    },
    solana_measure::{measure, measure::Measure},
    solana_metrics::{datapoint_error, inc_new_counter_debug},
    solana_program_runtime::timings::{ExecuteTimingType, ExecuteTimings, ThreadExecuteTimings},
    solana_rayon_threadlimit::{get_max_thread_count, get_thread_count},
    solana_runtime::{
        accounts_background_service::{AbsRequestSender, SnapshotRequestType},
        accounts_db::{AccountShrinkThreshold, AccountsDbConfig},
        accounts_index::AccountSecondaryIndexes,
        accounts_update_notifier_interface::AccountsUpdateNotifier,
        bank::{
            Bank, RentDebits, TransactionBalancesSet, TransactionExecutionDetails,
            TransactionExecutionResult, TransactionResults, VerifyBankHash,
        },
        bank_forks::BankForks,
        bank_utils,
        block_cost_limits::*,
        commitment::VOTE_THRESHOLD_SIZE,
        cost_model::CostModel,
        epoch_accounts_hash::EpochAccountsHash,
        prioritization_fee_cache::PrioritizationFeeCache,
        runtime_config::RuntimeConfig,
        transaction_batch::TransactionBatch,
        vote_account::VoteAccountsHashMap,
        vote_sender_types::ReplayVoteSender,
    },
    solana_sdk::{
        clock::{Slot, MAX_PROCESSING_AGE},
        feature_set,
        genesis_config::GenesisConfig,
        hash::Hash,
        pubkey::Pubkey,
        signature::{Keypair, Signature},
        timing,
        transaction::{
            Result, SanitizedTransaction, TransactionError, TransactionVerificationMode,
            VersionedTransaction,
        },
    },
    solana_transaction_status::token_balances::TransactionTokenBalancesSet,
    std::{
        borrow::Cow,
        collections::{HashMap, HashSet},
        path::PathBuf,
        result,
        sync::{
            atomic::{AtomicBool, Ordering::Relaxed},
            Arc, Mutex, RwLock,
        },
        time::{Duration, Instant},
    },
    thiserror::Error,
};

// it tracks the block cost available capacity - number of compute-units allowed
// by max block cost limit.
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

struct TransactionBatchWithIndexes<'a, 'b> {
    pub batch: TransactionBatch<'a, 'b>,
    pub transaction_indexes: Vec<usize>,
}

struct ReplayEntry {
    entry: EntryType,
    starting_index: usize,
}

// get_max_thread_count to match number of threads in the old code.
// see: https://github.com/solana-labs/solana/pull/24853
lazy_static! {
    static ref PAR_THREAD_POOL: ThreadPool = rayon::ThreadPoolBuilder::new()
        .num_threads(get_max_thread_count())
        .thread_name(|ix| format!("solBstoreProc{ix:02}"))
        .build()
        .unwrap();
}

fn first_err(results: &[Result<()>]) -> Result<()> {
    for r in results {
        if r.is_err() {
            return r.clone();
        }
    }
    Ok(())
}

// Includes transaction signature for unit-testing
fn get_first_error(
    batch: &TransactionBatch,
    fee_collection_results: Vec<Result<()>>,
) -> Option<(Result<()>, Signature)> {
    let mut first_err = None;
    for (result, transaction) in fee_collection_results
        .iter()
        .zip(batch.sanitized_transactions())
    {
        if let Err(ref err) = result {
            if first_err.is_none() {
                first_err = Some((result.clone(), *transaction.signature()));
            }
            warn!(
                "Unexpected validator error: {:?}, transaction: {:?}",
                err, transaction
            );
            datapoint_error!(
                "validator_process_entry_error",
                (
                    "error",
                    format!("error: {err:?}, transaction: {transaction:?}"),
                    String
                )
            );
        }
    }
    first_err
}

fn aggregate_total_execution_units(execute_timings: &ExecuteTimings) -> u64 {
    let mut execute_cost_units: u64 = 0;
    for (program_id, timing) in &execute_timings.details.per_program_timings {
        if timing.count < 1 {
            continue;
        }
        execute_cost_units =
            execute_cost_units.saturating_add(timing.accumulated_units / timing.count as u64);
        trace!("aggregated execution cost of {:?} {:?}", program_id, timing);
    }
    execute_cost_units
}

fn execute_batch(
    batch: &TransactionBatchWithIndexes,
    bank: &Arc<Bank>,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    timings: &mut ExecuteTimings,
    cost_capacity_meter: Arc<RwLock<BlockCostCapacityMeter>>,
    tx_cost: u64,
    log_messages_bytes_limit: Option<usize>,
) -> Result<()> {
    let TransactionBatchWithIndexes {
        batch,
        transaction_indexes,
    } = batch;
    let record_token_balances = transaction_status_sender.is_some();

    let mut mint_decimals: HashMap<Pubkey, u8> = HashMap::new();

    let pre_token_balances = if record_token_balances {
        collect_token_balances(bank, batch, &mut mint_decimals)
    } else {
        vec![]
    };

    let pre_process_units: u64 = aggregate_total_execution_units(timings);

    let (tx_results, balances) = batch.bank().load_execute_and_commit_transactions(
        batch,
        MAX_PROCESSING_AGE,
        transaction_status_sender.is_some(),
        transaction_status_sender.is_some(),
        transaction_status_sender.is_some(),
        transaction_status_sender.is_some(),
        timings,
        log_messages_bytes_limit,
    );

    if bank
        .feature_set
        .is_active(&feature_set::gate_large_block::id())
    {
        let execution_cost_units = aggregate_total_execution_units(timings) - pre_process_units;
        let remaining_block_cost_cap = cost_capacity_meter
            .write()
            .unwrap()
            .accumulate(execution_cost_units + tx_cost);

        debug!(
            "bank {} executed a batch, number of transactions {}, total execute cu {}, total additional cu {}, remaining block cost cap {}",
            bank.slot(),
            batch.sanitized_transactions().len(),
            execution_cost_units,
            tx_cost,
            remaining_block_cost_cap,
        );

        if remaining_block_cost_cap == 0_u64 {
            return Err(TransactionError::WouldExceedMaxBlockCostLimit);
        }
    }

    bank_utils::find_and_send_votes(
        batch.sanitized_transactions(),
        &tx_results,
        replay_vote_sender,
    );

    let TransactionResults {
        fee_collection_results,
        execution_results,
        rent_debits,
        ..
    } = tx_results;

    if let Some(transaction_status_sender) = transaction_status_sender {
        let transactions = batch.sanitized_transactions().to_vec();
        let post_token_balances = if record_token_balances {
            collect_token_balances(bank, batch, &mut mint_decimals)
        } else {
            vec![]
        };

        let token_balances =
            TransactionTokenBalancesSet::new(pre_token_balances, post_token_balances);

        transaction_status_sender.send_transaction_status_batch(
            bank.clone(),
            transactions,
            execution_results,
            balances,
            token_balances,
            rent_debits,
            transaction_indexes.to_vec(),
        );
    }

    let first_err = get_first_error(batch, fee_collection_results);
    first_err.map(|(result, _)| result).unwrap_or(Ok(()))
}

#[derive(Default)]
struct ExecuteBatchesInternalMetrics {
    execution_timings_per_thread: HashMap<usize, ThreadExecuteTimings>,
    total_batches_len: u64,
    execute_batches_us: u64,
}

fn execute_batches_internal(
    bank: &Arc<Bank>,
    batches: &[TransactionBatchWithIndexes],
    entry_callback: Option<&ProcessCallback>,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    cost_capacity_meter: Arc<RwLock<BlockCostCapacityMeter>>,
    tx_costs: &[u64],
    log_messages_bytes_limit: Option<usize>,
) -> Result<ExecuteBatchesInternalMetrics> {
    assert!(!batches.is_empty());
    inc_new_counter_debug!("bank-par_execute_entries-count", batches.len());
    let execution_timings_per_thread: Mutex<HashMap<usize, ThreadExecuteTimings>> =
        Mutex::new(HashMap::new());

    let mut execute_batches_elapsed = Measure::start("execute_batches_elapsed");
    let results: Vec<Result<()>> = PAR_THREAD_POOL.install(|| {
        batches
            .into_par_iter()
            .enumerate()
            .map(|(index, transaction_batch_with_indexes)| {
                let transaction_count = transaction_batch_with_indexes
                    .batch
                    .sanitized_transactions()
                    .len() as u64;
                let mut timings = ExecuteTimings::default();
                let (result, execute_batches_time): (Result<()>, Measure) = measure!(
                    {
                        let result = execute_batch(
                            transaction_batch_with_indexes,
                            bank,
                            transaction_status_sender,
                            replay_vote_sender,
                            &mut timings,
                            cost_capacity_meter.clone(),
                            tx_costs[index],
                            log_messages_bytes_limit,
                        );
                        if let Some(entry_callback) = entry_callback {
                            entry_callback(bank);
                        }
                        result
                    },
                    "execute_batch",
                );

                let thread_index = PAR_THREAD_POOL.current_thread_index().unwrap();
                execution_timings_per_thread
                    .lock()
                    .unwrap()
                    .entry(thread_index)
                    .and_modify(|thread_execution_time| {
                        let ThreadExecuteTimings {
                            total_thread_us,
                            total_transactions_executed,
                            execute_timings: total_thread_execute_timings,
                        } = thread_execution_time;
                        *total_thread_us += execute_batches_time.as_us();
                        *total_transactions_executed += transaction_count;
                        total_thread_execute_timings
                            .saturating_add_in_place(ExecuteTimingType::TotalBatchesLen, 1);
                        total_thread_execute_timings.accumulate(&timings);
                    })
                    .or_insert(ThreadExecuteTimings {
                        total_thread_us: execute_batches_time.as_us(),
                        total_transactions_executed: transaction_count,
                        execute_timings: timings,
                    });
                result
            })
            .collect()
    });
    execute_batches_elapsed.stop();

    first_err(&results)?;

    Ok(ExecuteBatchesInternalMetrics {
        execution_timings_per_thread: execution_timings_per_thread.into_inner().unwrap(),
        total_batches_len: batches.len() as u64,
        execute_batches_us: execute_batches_elapsed.as_us(),
    })
}

fn rebatch_transactions<'a>(
    lock_results: &'a [Result<()>],
    bank: &'a Arc<Bank>,
    sanitized_txs: &'a [SanitizedTransaction],
    start: usize,
    end: usize,
    transaction_indexes: &'a [usize],
) -> TransactionBatchWithIndexes<'a, 'a> {
    let txs = &sanitized_txs[start..=end];
    let results = &lock_results[start..=end];
    let mut tx_batch = TransactionBatch::new(results.to_vec(), bank, Cow::from(txs));
    tx_batch.set_needs_unlock(false);

    let transaction_indexes = transaction_indexes[start..=end].to_vec();
    TransactionBatchWithIndexes {
        batch: tx_batch,
        transaction_indexes,
    }
}

fn execute_batches(
    bank: &Arc<Bank>,
    batches: &[TransactionBatchWithIndexes],
    entry_callback: Option<&ProcessCallback>,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    confirmation_timing: &mut ConfirmationTiming,
    cost_capacity_meter: Arc<RwLock<BlockCostCapacityMeter>>,
    log_messages_bytes_limit: Option<usize>,
) -> Result<()> {
    if batches.is_empty() {
        return Ok(());
    }

    let ((lock_results, sanitized_txs), transaction_indexes): ((Vec<_>, Vec<_>), Vec<_>) = batches
        .iter()
        .flat_map(|batch| {
            batch
                .batch
                .lock_results()
                .iter()
                .cloned()
                .zip(batch.batch.sanitized_transactions().to_vec())
                .zip(batch.transaction_indexes.to_vec())
        })
        .unzip();

    let mut minimal_tx_cost = u64::MAX;
    let mut total_cost: u64 = 0;
    let mut total_cost_without_bpf: u64 = 0;
    // Allowing collect here, since it also computes the minimal tx cost, and aggregate cost.
    // These two values are later used for checking if the tx_costs vector needs to be iterated over.
    // The collection is a pair of (full cost, cost without estimated-bpf-code-costs).
    #[allow(clippy::needless_collect)]
    let tx_costs = sanitized_txs
        .iter()
        .map(|tx| {
            let tx_cost = CostModel::calculate_cost(tx, &bank.feature_set);
            let cost = tx_cost.sum();
            let cost_without_bpf = tx_cost.sum_without_bpf();
            minimal_tx_cost = std::cmp::min(minimal_tx_cost, cost);
            total_cost = total_cost.saturating_add(cost);
            total_cost_without_bpf = total_cost_without_bpf.saturating_add(cost_without_bpf);
            (cost, cost_without_bpf)
        })
        .collect::<Vec<_>>();

    let target_batch_count = get_thread_count() as u64;

    let mut tx_batches: Vec<TransactionBatchWithIndexes> = vec![];
    let mut tx_batch_costs: Vec<u64> = vec![];
    let rebatched_txs = if total_cost > target_batch_count.saturating_mul(minimal_tx_cost) {
        let target_batch_cost = total_cost / target_batch_count;
        let mut batch_cost: u64 = 0;
        let mut batch_cost_without_bpf: u64 = 0;
        let mut slice_start = 0;
        tx_costs
            .into_iter()
            .enumerate()
            .for_each(|(index, cost_pair)| {
                let next_index = index + 1;
                batch_cost = batch_cost.saturating_add(cost_pair.0);
                batch_cost_without_bpf = batch_cost_without_bpf.saturating_add(cost_pair.1);
                if batch_cost >= target_batch_cost || next_index == sanitized_txs.len() {
                    let tx_batch = rebatch_transactions(
                        &lock_results,
                        bank,
                        &sanitized_txs,
                        slice_start,
                        index,
                        &transaction_indexes,
                    );
                    slice_start = next_index;
                    tx_batches.push(tx_batch);
                    tx_batch_costs.push(batch_cost_without_bpf);
                    batch_cost = 0;
                    batch_cost_without_bpf = 0;
                }
            });
        &tx_batches[..]
    } else {
        // Ensure that the total cost attributed to this batch is essentially correct
        // batches.is_empty() check at top of function ensures that n > 0 for following divide
        let n = batches.len();
        tx_batch_costs = vec![total_cost_without_bpf / (n as u64); n];
        batches
    };

    let execute_batches_internal_metrics = execute_batches_internal(
        bank,
        rebatched_txs,
        entry_callback,
        transaction_status_sender,
        replay_vote_sender,
        cost_capacity_meter,
        &tx_batch_costs,
        log_messages_bytes_limit,
    )?;

    confirmation_timing.process_execute_batches_internal_metrics(execute_batches_internal_metrics);
    Ok(())
}

/// Process an ordered list of entries in parallel
/// 1. In order lock accounts for each entry while the lock succeeds, up to a Tick entry
/// 2. Process the locked group in parallel
/// 3. Register the `Tick` if it's available
/// 4. Update the leader scheduler, goto 1
///
/// This method is for use testing against a single Bank, and assumes `Bank::transaction_count()`
/// represents the number of transactions executed in this Bank
pub fn process_entries_for_tests(
    bank: &Arc<Bank>,
    entries: Vec<Entry>,
    randomize: bool,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
) -> Result<()> {
    let verify_transaction = {
        let bank = bank.clone();
        move |versioned_tx: VersionedTransaction| -> Result<SanitizedTransaction> {
            bank.verify_transaction(versioned_tx, TransactionVerificationMode::FullVerification)
        }
    };

    let mut entry_starting_index: usize = bank.transaction_count().try_into().unwrap();
    let mut confirmation_timing = ConfirmationTiming::default();
    let mut replay_entries: Vec<_> =
        entry::verify_transactions(entries, Arc::new(verify_transaction))?
            .into_iter()
            .map(|entry| {
                let starting_index = entry_starting_index;
                if let EntryType::Transactions(ref transactions) = entry {
                    entry_starting_index = entry_starting_index.saturating_add(transactions.len());
                }
                ReplayEntry {
                    entry,
                    starting_index,
                }
            })
            .collect();

    let _ignored_prioritization_fee_cache = PrioritizationFeeCache::new(0u64);
    let result = process_entries_with_callback(
        bank,
        &mut replay_entries,
        randomize,
        None,
        transaction_status_sender,
        replay_vote_sender,
        &mut confirmation_timing,
        Arc::new(RwLock::new(BlockCostCapacityMeter::default())),
        None,
        &_ignored_prioritization_fee_cache,
    );

    debug!("process_entries: {:?}", confirmation_timing);
    result
}

// Note: If randomize is true this will shuffle entries' transactions in-place.
#[allow(clippy::too_many_arguments)]
fn process_entries_with_callback(
    bank: &Arc<Bank>,
    entries: &mut [ReplayEntry],
    randomize: bool,
    entry_callback: Option<&ProcessCallback>,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    confirmation_timing: &mut ConfirmationTiming,
    cost_capacity_meter: Arc<RwLock<BlockCostCapacityMeter>>,
    log_messages_bytes_limit: Option<usize>,
    prioritization_fee_cache: &PrioritizationFeeCache,
) -> Result<()> {
    // accumulator for entries that can be processed in parallel
    let mut batches = vec![];
    let mut tick_hashes = vec![];
    let mut rng = thread_rng();

    for ReplayEntry {
        entry,
        starting_index,
    } in entries
    {
        match entry {
            EntryType::Tick(hash) => {
                // If it's a tick, save it for later
                tick_hashes.push(hash);
                if bank.is_block_boundary(bank.tick_height() + tick_hashes.len() as u64) {
                    // If it's a tick that will cause a new blockhash to be created,
                    // execute the group and register the tick
                    execute_batches(
                        bank,
                        &batches,
                        entry_callback,
                        transaction_status_sender,
                        replay_vote_sender,
                        confirmation_timing,
                        cost_capacity_meter.clone(),
                        log_messages_bytes_limit,
                    )?;
                    batches.clear();
                    for hash in &tick_hashes {
                        bank.register_tick(hash);
                    }
                    tick_hashes.clear();
                }
            }
            EntryType::Transactions(transactions) => {
                let starting_index = *starting_index;
                let transaction_indexes = if randomize {
                    let mut transactions_and_indexes: Vec<(SanitizedTransaction, usize)> =
                        transactions.drain(..).zip(starting_index..).collect();
                    transactions_and_indexes.shuffle(&mut rng);
                    let (txs, indexes): (Vec<_>, Vec<_>) =
                        transactions_and_indexes.into_iter().unzip();
                    *transactions = txs;
                    indexes
                } else {
                    (starting_index..starting_index.saturating_add(transactions.len())).collect()
                };

                loop {
                    // try to lock the accounts
                    let batch = bank.prepare_sanitized_batch(transactions);
                    let first_lock_err = first_err(batch.lock_results());

                    // if locking worked
                    if first_lock_err.is_ok() {
                        batches.push(TransactionBatchWithIndexes {
                            batch,
                            transaction_indexes,
                        });
                        // entry is scheduled to be processed, transactions in it can be used to
                        // update prioritization fee cache asynchronously.
                        prioritization_fee_cache.update(bank.clone(), transactions.iter());
                        // done with this entry
                        break;
                    }
                    // else we failed to lock, 2 possible reasons
                    if batches.is_empty() {
                        // An entry has account lock conflicts with *itself*, which should not happen
                        // if generated by a properly functioning leader
                        datapoint_error!(
                            "validator_process_entry_error",
                            (
                                "error",
                                format!(
                                    "Lock accounts error, entry conflicts with itself, txs: {transactions:?}"
                                ),
                                String
                            )
                        );
                        // bail
                        first_lock_err?;
                    } else {
                        // else we have an entry that conflicts with a prior entry
                        // execute the current queue and try to process this entry again
                        execute_batches(
                            bank,
                            &batches,
                            entry_callback,
                            transaction_status_sender,
                            replay_vote_sender,
                            confirmation_timing,
                            cost_capacity_meter.clone(),
                            log_messages_bytes_limit,
                        )?;
                        batches.clear();
                    }
                }
            }
        }
    }
    execute_batches(
        bank,
        &batches,
        entry_callback,
        transaction_status_sender,
        replay_vote_sender,
        confirmation_timing,
        cost_capacity_meter,
        log_messages_bytes_limit,
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

/// Callback for accessing bank state while processing the blockstore
pub type ProcessCallback = Arc<dyn Fn(&Bank) + Sync + Send>;

#[derive(Default, Clone)]
pub struct ProcessOptions {
    pub poh_verify: bool,
    pub full_leader_cache: bool,
    pub halt_at_slot: Option<Slot>,
    pub entry_callback: Option<ProcessCallback>,
    pub new_hard_forks: Option<Vec<Slot>>,
    pub debug_keys: Option<Arc<HashSet<Pubkey>>>,
    pub account_indexes: AccountSecondaryIndexes,
    pub limit_load_slot_count_from_snapshot: Option<usize>,
    pub allow_dead_slots: bool,
    pub accounts_db_test_hash_calculation: bool,
    pub accounts_db_skip_shrink: bool,
    pub accounts_db_config: Option<AccountsDbConfig>,
    pub verify_index: bool,
    pub shrink_ratio: AccountShrinkThreshold,
    pub runtime_config: RuntimeConfig,
    pub on_halt_store_hash_raw_data_for_debug: bool,
    /// true if after processing the contents of the blockstore at startup, we should run an accounts hash calc
    /// This is useful for debugging.
    pub run_final_accounts_hash_calc: bool,
}

pub fn test_process_blockstore(
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    opts: &ProcessOptions,
    exit: &Arc<AtomicBool>,
) -> (Arc<RwLock<BankForks>>, LeaderScheduleCache) {
    // Spin up a thread to be a fake Accounts Background Service.  Need to intercept and handle all
    // EpochAccountsHash requests so future rooted banks do not hang in Bank::freeze() waiting for
    // an in-flight EAH calculation to complete.
    let (snapshot_request_sender, snapshot_request_receiver) = crossbeam_channel::unbounded();
    let abs_request_sender = AbsRequestSender::new(snapshot_request_sender);
    let bg_exit = Arc::new(AtomicBool::new(false));
    let bg_thread = {
        let exit = Arc::clone(&bg_exit);
        std::thread::spawn(move || {
            while !exit.load(Relaxed) {
                snapshot_request_receiver
                    .try_iter()
                    .filter(|snapshot_request| {
                        snapshot_request.request_type == SnapshotRequestType::EpochAccountsHash
                    })
                    .for_each(|snapshot_request| {
                        snapshot_request
                            .snapshot_root_bank
                            .rc
                            .accounts
                            .accounts_db
                            .epoch_accounts_hash_manager
                            .set_valid(
                                EpochAccountsHash::new(Hash::new_unique()),
                                snapshot_request.snapshot_root_bank.slot(),
                            )
                    });
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };

    let (bank_forks, leader_schedule_cache, ..) = crate::bank_forks_utils::load_bank_forks(
        genesis_config,
        blockstore,
        Vec::new(),
        None,
        None,
        opts,
        None,
        None,
        exit,
    );

    process_blockstore_from_root(
        blockstore,
        &bank_forks,
        &leader_schedule_cache,
        opts,
        None,
        None,
        &abs_request_sender,
    )
    .unwrap();

    bg_exit.store(true, Relaxed);
    bg_thread.join().unwrap();

    (bank_forks, leader_schedule_cache)
}

pub(crate) fn process_blockstore_for_bank_0(
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    account_paths: Vec<PathBuf>,
    opts: &ProcessOptions,
    cache_block_meta_sender: Option<&CacheBlockMetaSender>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: &Arc<AtomicBool>,
) -> Arc<RwLock<BankForks>> {
    // Setup bank for slot 0
    let bank0 = Bank::new_with_paths(
        genesis_config,
        Arc::new(opts.runtime_config.clone()),
        account_paths,
        opts.debug_keys.clone(),
        Some(&crate::builtins::get(opts.runtime_config.bpf_jit)),
        opts.account_indexes.clone(),
        opts.shrink_ratio,
        false,
        opts.accounts_db_config.clone(),
        accounts_update_notifier,
        exit,
    );
    let bank_forks = Arc::new(RwLock::new(BankForks::new(bank0)));

    info!("Processing ledger for slot 0...");
    process_bank_0(
        &bank_forks.read().unwrap().root_bank(),
        blockstore,
        opts,
        &VerifyRecyclers::default(),
        cache_block_meta_sender,
    );
    bank_forks
}

/// Process blockstore from a known root bank
#[allow(clippy::too_many_arguments)]
pub fn process_blockstore_from_root(
    blockstore: &Blockstore,
    bank_forks: &RwLock<BankForks>,
    leader_schedule_cache: &LeaderScheduleCache,
    opts: &ProcessOptions,
    transaction_status_sender: Option<&TransactionStatusSender>,
    cache_block_meta_sender: Option<&CacheBlockMetaSender>,
    accounts_background_request_sender: &AbsRequestSender,
) -> result::Result<(), BlockstoreProcessorError> {
    // Starting slot must be a root, and thus has no parents
    assert_eq!(bank_forks.read().unwrap().banks().len(), 1);
    let bank = bank_forks.read().unwrap().root_bank();
    assert!(bank.parent().is_none());

    let start_slot = bank.slot();
    info!("Processing ledger from slot {}...", start_slot);
    let now = Instant::now();

    // ensure start_slot is rooted for correct replay
    if blockstore.is_primary_access() {
        blockstore
            .mark_slots_as_if_rooted_normally_at_startup(
                vec![(bank.slot(), Some(bank.hash()))],
                true,
            )
            .expect("Couldn't mark start_slot as root on startup");
    } else {
        info!(
            "Starting slot {} isn't root and won't be updated due to being secondary blockstore access",
            start_slot
        );
    }

    if let Ok(Some(highest_slot)) = blockstore.highest_slot() {
        info!("ledger holds data through slot {}", highest_slot);
    }

    let mut timing = ExecuteTimings::default();

    // Iterate and replay slots from blockstore starting from `start_slot`
    let mut num_slots_processed = 0;
    if let Some(start_slot_meta) = blockstore
        .meta(start_slot)
        .unwrap_or_else(|_| panic!("Failed to get meta for slot {start_slot}"))
    {
        num_slots_processed = load_frozen_forks(
            bank_forks,
            start_slot,
            &start_slot_meta,
            blockstore,
            leader_schedule_cache,
            opts,
            transaction_status_sender,
            cache_block_meta_sender,
            &mut timing,
            accounts_background_request_sender,
        )?;
    } else {
        // If there's no meta in the blockstore for the input `start_slot`,
        // then we started from a snapshot and are unable to process anything.
        //
        // If the ledger has any data at all, the snapshot was likely taken at
        // a slot that is not within the range of ledger min/max slot(s).
        warn!(
            "Starting slot {} is not in Blockstore, unable to process",
            start_slot
        );
    };

    let processing_time = now.elapsed();

    datapoint_info!(
        "process_blockstore_from_root",
        ("total_time_us", processing_time.as_micros(), i64),
        (
            "frozen_banks",
            bank_forks.read().unwrap().frozen_banks().len(),
            i64
        ),
        ("slot", bank_forks.read().unwrap().root(), i64),
        ("num_slots_processed", num_slots_processed, i64),
        ("forks", bank_forks.read().unwrap().banks().len(), i64),
    );

    info!("ledger processing timing: {:?}", timing);
    {
        let bank_forks = bank_forks.read().unwrap();
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
        assert!(bank_forks.active_bank_slots().is_empty());
    }

    Ok(())
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
) -> result::Result<(), BlockstoreProcessorError> {
    let mut confirmation_timing = ConfirmationTiming::default();
    let skip_verification = !opts.poh_verify;
    let _ignored_prioritization_fee_cache = PrioritizationFeeCache::new(0u64);

    confirm_slot(
        blockstore,
        bank,
        &mut confirmation_timing,
        progress,
        skip_verification,
        transaction_status_sender,
        replay_vote_sender,
        opts.entry_callback.as_ref(),
        recyclers,
        opts.allow_dead_slots,
        opts.runtime_config.log_messages_bytes_limit,
        &_ignored_prioritization_fee_cache,
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

#[derive(Debug)]
pub struct ConfirmationTiming {
    pub started: Instant,
    pub replay_elapsed: u64,
    pub execute_batches_us: u64,
    pub poh_verify_elapsed: u64,
    pub transaction_verify_elapsed: u64,
    pub fetch_elapsed: u64,
    pub fetch_fail_elapsed: u64,
    pub execute_timings: ExecuteTimings,
    pub end_to_end_execute_timings: ThreadExecuteTimings,
}

impl ConfirmationTiming {
    fn process_execute_batches_internal_metrics(
        &mut self,
        execute_batches_internal_metrics: ExecuteBatchesInternalMetrics,
    ) {
        let ConfirmationTiming {
            execute_timings: ref mut cumulative_execute_timings,
            execute_batches_us: ref mut cumulative_execute_batches_us,
            ref mut end_to_end_execute_timings,
            ..
        } = self;

        saturating_add_assign!(
            *cumulative_execute_batches_us,
            execute_batches_internal_metrics.execute_batches_us
        );

        cumulative_execute_timings.saturating_add_in_place(
            ExecuteTimingType::TotalBatchesLen,
            execute_batches_internal_metrics.total_batches_len,
        );
        cumulative_execute_timings.saturating_add_in_place(ExecuteTimingType::NumExecuteBatches, 1);

        let mut current_max_thread_execution_time: Option<ThreadExecuteTimings> = None;
        for (_, thread_execution_time) in execute_batches_internal_metrics
            .execution_timings_per_thread
            .into_iter()
        {
            let ThreadExecuteTimings {
                total_thread_us,
                execute_timings,
                ..
            } = &thread_execution_time;
            cumulative_execute_timings.accumulate(execute_timings);
            if *total_thread_us
                > current_max_thread_execution_time
                    .as_ref()
                    .map(|thread_execution_time| thread_execution_time.total_thread_us)
                    .unwrap_or(0)
            {
                current_max_thread_execution_time = Some(thread_execution_time);
            }
        }

        if let Some(current_max_thread_execution_time) = current_max_thread_execution_time {
            end_to_end_execute_timings.accumulate(&current_max_thread_execution_time);
            end_to_end_execute_timings
                .execute_timings
                .saturating_add_in_place(ExecuteTimingType::NumExecuteBatches, 1);
        };
    }
}

impl Default for ConfirmationTiming {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            replay_elapsed: 0,
            execute_batches_us: 0,
            poh_verify_elapsed: 0,
            transaction_verify_elapsed: 0,
            fetch_elapsed: 0,
            fetch_fail_elapsed: 0,
            execute_timings: ExecuteTimings::default(),
            end_to_end_execute_timings: ThreadExecuteTimings::default(),
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
    entry_callback: Option<&ProcessCallback>,
    recyclers: &VerifyRecyclers,
    allow_dead_slots: bool,
    log_messages_bytes_limit: Option<usize>,
    prioritization_fee_cache: &PrioritizationFeeCache,
) -> result::Result<(), BlockstoreProcessorError> {
    let slot = bank.slot();

    let slot_entries_load_result = {
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

    confirm_slot_entries(
        bank,
        slot_entries_load_result,
        timing,
        progress,
        skip_verification,
        transaction_status_sender,
        replay_vote_sender,
        entry_callback,
        recyclers,
        log_messages_bytes_limit,
        prioritization_fee_cache,
    )
}

#[allow(clippy::too_many_arguments)]
fn confirm_slot_entries(
    bank: &Arc<Bank>,
    slot_entries_load_result: (Vec<Entry>, u64, bool),
    timing: &mut ConfirmationTiming,
    progress: &mut ConfirmationProgress,
    skip_verification: bool,
    transaction_status_sender: Option<&TransactionStatusSender>,
    replay_vote_sender: Option<&ReplayVoteSender>,
    entry_callback: Option<&ProcessCallback>,
    recyclers: &VerifyRecyclers,
    log_messages_bytes_limit: Option<usize>,
    prioritization_fee_cache: &PrioritizationFeeCache,
) -> result::Result<(), BlockstoreProcessorError> {
    let slot = bank.slot();
    let (entries, num_shreds, slot_full) = slot_entries_load_result;
    let num_entries = entries.len();
    let mut entry_starting_indexes = Vec::with_capacity(num_entries);
    let mut entry_starting_index = progress.num_txs;
    let num_txs = entries
        .iter()
        .map(|e| {
            let num_txs = e.transactions.len();
            let next_starting_index = entry_starting_index.saturating_add(num_txs);
            entry_starting_indexes.push(entry_starting_index);
            entry_starting_index = next_starting_index;
            num_txs
        })
        .sum::<usize>();
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
            let cost_capacity_meter = Arc::new(RwLock::new(BlockCostCapacityMeter::default()));
            let mut replay_entries: Vec<_> = entries
                .unwrap()
                .into_iter()
                .zip(entry_starting_indexes)
                .map(|(entry, starting_index)| ReplayEntry {
                    entry,
                    starting_index,
                })
                .collect();
            // Note: This will shuffle entries' transactions in-place.
            let process_result = process_entries_with_callback(
                bank,
                &mut replay_entries,
                true, // shuffle transactions.
                entry_callback,
                transaction_status_sender,
                replay_vote_sender,
                timing,
                cost_capacity_meter,
                log_messages_bytes_limit,
                prioritization_fee_cache,
            )
            .map_err(BlockstoreProcessorError::from);
            replay_elapsed.stop();
            timing.replay_elapsed += replay_elapsed.as_us();

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
    )
    .expect("Failed to process bank 0 from ledger. Did you forget to provide a snapshot?");
    bank0.freeze();
    if blockstore.is_primary_access() {
        blockstore.insert_bank_hash(bank0.slot(), bank0.hash(), false);
    }
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
    halt_at_slot: Option<Slot>,
) -> result::Result<(), BlockstoreProcessorError> {
    if meta.next_slots.is_empty() {
        return Ok(());
    }

    // This is a fork point if there are multiple children, create a new child bank for each fork
    for next_slot in &meta.next_slots {
        let skip_next_slot = halt_at_slot
            .map(|halt_at_slot| *next_slot > halt_at_slot)
            .unwrap_or(false);
        if skip_next_slot {
            continue;
        }

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
    bank_forks: &RwLock<BankForks>,
    start_slot: Slot,
    start_slot_meta: &SlotMeta,
    blockstore: &Blockstore,
    leader_schedule_cache: &LeaderScheduleCache,
    opts: &ProcessOptions,
    transaction_status_sender: Option<&TransactionStatusSender>,
    cache_block_meta_sender: Option<&CacheBlockMetaSender>,
    timing: &mut ExecuteTimings,
    accounts_background_request_sender: &AbsRequestSender,
) -> result::Result<u64, BlockstoreProcessorError> {
    let recyclers = VerifyRecyclers::default();
    let mut all_banks = HashMap::new();
    let mut last_status_report = Instant::now();
    let mut pending_slots = vec![];
    // The total number of slots processed
    let mut total_slots_elapsed = 0;
    // The number of slots processed between status report updates
    let mut slots_elapsed = 0;
    let mut txs = 0;
    let blockstore_max_root = blockstore.max_root();
    let mut root = bank_forks.read().unwrap().root();
    let max_root = std::cmp::max(root, blockstore_max_root);

    info!(
        "load_frozen_forks() latest root from blockstore: {}, max_root: {}",
        blockstore_max_root, max_root,
    );

    process_next_slots(
        &bank_forks.read().unwrap().get(start_slot).unwrap(),
        start_slot_meta,
        blockstore,
        leader_schedule_cache,
        &mut pending_slots,
        opts.halt_at_slot,
    )?;

    let on_halt_store_hash_raw_data_for_debug = opts.on_halt_store_hash_raw_data_for_debug;
    if Some(bank_forks.read().unwrap().root()) != opts.halt_at_slot {
        let mut set_root_us = 0;
        let mut root_retain_us = 0;
        let mut process_single_slot_us = 0;
        let mut voting_us = 0;
        while !pending_slots.is_empty() {
            timing.details.per_program_timings.clear();
            let (meta, bank, last_entry_hash) = pending_slots.pop().unwrap();
            let slot = bank.slot();
            if last_status_report.elapsed() > Duration::from_secs(2) {
                let secs = last_status_report.elapsed().as_secs() as f32;
                last_status_report = Instant::now();
                info!(
                    "processing ledger: slot={}, last root slot={} slots={} slots/s={:?} txs/s={}, set_root_us={set_root_us}, root_retain_us={root_retain_us}, process_single_slot_us:{process_single_slot_us}, voting_us: {voting_us}",
                    slot,
                    root,
                    slots_elapsed,
                    slots_elapsed as f32 / secs,
                    txs as f32 / secs,
                );
                slots_elapsed = 0;
                txs = 0;
                set_root_us = 0;
                root_retain_us = 0;
                process_single_slot_us = 0;
                voting_us = 0;
            }

            let mut progress = ConfirmationProgress::new(last_entry_hash);

            let mut m = Measure::start("process_single_slot");
            let bank = bank_forks.write().unwrap().insert(bank);
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
            )
            .is_err()
            {
                assert!(bank_forks.write().unwrap().remove(bank.slot()).is_some());
                continue;
            }
            txs += progress.num_txs;

            // Block must be frozen by this point, otherwise `process_single_slot` would
            // have errored above
            assert!(bank.is_frozen());
            all_banks.insert(bank.slot(), bank.clone());
            m.stop();
            process_single_slot_us += m.as_us();

            let mut m = Measure::start("voting");

            // If we've reached the last known root in blockstore, start looking
            // for newer cluster confirmed roots
            let new_root_bank = {
                if bank_forks.read().unwrap().root() >= max_root {
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

                                rooted_slots.push((new_root_bank.slot(), Some(new_root_bank.hash())));
                                // As noted, the cluster confirmed root should be descended from
                                // our last root; therefore parent should be set
                                new_root_bank = new_root_bank.parent().unwrap();
                            }
                            inc_new_counter_info!("load_frozen_forks-cluster-confirmed-root", rooted_slots.len());
                            if blockstore.is_primary_access() {
                                blockstore
                                    .mark_slots_as_if_rooted_normally_at_startup(rooted_slots, true)
                                    .expect("Blockstore::mark_slots_as_if_rooted_normally_at_startup() should succeed");
                            }
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

            m.stop();
            voting_us += m.as_us();

            if let Some(new_root_bank) = new_root_bank {
                let mut m = Measure::start("set_root");
                root = new_root_bank.slot();

                leader_schedule_cache.set_root(new_root_bank);
                let _ = bank_forks.write().unwrap().set_root(
                    root,
                    accounts_background_request_sender,
                    None,
                );
                m.stop();
                set_root_us += m.as_us();

                // Filter out all non descendants of the new root
                let mut m = Measure::start("filter pending slots");
                pending_slots
                    .retain(|(_, pending_bank, _)| pending_bank.ancestors.contains_key(&root));
                all_banks.retain(|_, bank| bank.ancestors.contains_key(&root));
                m.stop();
                root_retain_us += m.as_us();
            }

            slots_elapsed += 1;
            total_slots_elapsed += 1;

            trace!(
                "Bank for {}slot {} is complete",
                if root == slot { "root " } else { "" },
                slot,
            );

            let done_processing = opts
                .halt_at_slot
                .map(|halt_at_slot| slot >= halt_at_slot)
                .unwrap_or(false);
            if done_processing {
                if opts.run_final_accounts_hash_calc {
                    run_final_hash_calc(&bank, on_halt_store_hash_raw_data_for_debug);
                }
                break;
            }

            process_next_slots(
                &bank,
                &meta,
                blockstore,
                leader_schedule_cache,
                &mut pending_slots,
                opts.halt_at_slot,
            )?;
        }
    } else if on_halt_store_hash_raw_data_for_debug {
        run_final_hash_calc(
            &bank_forks.read().unwrap().root_bank(),
            on_halt_store_hash_raw_data_for_debug,
        );
    }

    Ok(total_slots_elapsed)
}

fn run_final_hash_calc(bank: &Bank, on_halt_store_hash_raw_data_for_debug: bool) {
    bank.force_flush_accounts_cache();
    // note that this slot may not be a root
    let _ = bank.verify_bank_hash(VerifyBankHash {
        test_hash_calculation: false,
        ignore_mismatch: true,
        require_rooted_bank: false,
        run_in_background: false,
        store_hash_raw_data_for_debug: on_halt_store_hash_raw_data_for_debug,
    });
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
    vote_accounts: &VoteAccountsHashMap,
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
) -> result::Result<(), BlockstoreProcessorError> {
    // Mark corrupt slots as dead so validators don't replay this slot and
    // see AlreadyProcessed errors later in ReplayStage
    confirm_full_slot(
        blockstore,
        bank,
        opts,
        recyclers,
        progress,
        transaction_status_sender,
        replay_vote_sender,
        timing,
    )
    .map_err(|err| {
        let slot = bank.slot();
        warn!("slot {} failed to verify: {}", slot, err);
        if blockstore.is_primary_access() {
            blockstore
                .set_dead_slot(slot)
                .expect("Failed to mark slot as dead in blockstore");
        } else {
            info!(
                "Failed slot {} won't be marked dead due to being secondary blockstore access",
                slot
            );
        }
        err
    })?;

    bank.freeze(); // all banks handled by this routine are created from complete slots
    if blockstore.is_primary_access() {
        blockstore.insert_bank_hash(bank.slot(), bank.hash(), false);
    }
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
    pub transaction_indexes: Vec<usize>,
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
        transaction_indexes: Vec<usize>,
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
                transaction_indexes,
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
        crate::{
            blockstore_options::{AccessType, BlockstoreOptions},
            genesis_utils::{
                create_genesis_config, create_genesis_config_with_leader, GenesisConfigInfo,
            },
        },
        matches::assert_matches,
        rand::{thread_rng, Rng},
        solana_entry::entry::{create_ticks, next_entry, next_entry_mut},
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
            native_token::LAMPORTS_PER_SOL,
            pubkey::Pubkey,
            signature::{Keypair, Signer},
            system_instruction::SystemError,
            system_transaction,
            transaction::{Transaction, TransactionError},
        },
        solana_vote_program::{
            self,
            vote_state::{VoteState, VoteStateVersions, MAX_LOCKOUT_HISTORY},
            vote_transaction,
        },
        std::{collections::BTreeSet, sync::RwLock},
        trees::tr,
    };

    // Convenience wrapper to optionally process blockstore with Secondary access.
    //
    // Setting up the ledger for a test requires Primary access as items will need to be inserted.
    // However, once a Secondary access has been opened, it won't automaticaly see updates made by
    // the Primary access. So, open (and close) the Secondary access within this function to ensure
    // that "stale" Secondary accesses don't propagate.
    fn test_process_blockstore_with_custom_options(
        genesis_config: &GenesisConfig,
        blockstore: &Blockstore,
        opts: &ProcessOptions,
        access_type: AccessType,
    ) -> (Arc<RwLock<BankForks>>, LeaderScheduleCache) {
        match access_type {
            AccessType::Primary | AccessType::PrimaryForMaintenance => {
                // Attempting to open a second Primary access would fail, so
                // just pass the original session if it is a Primary variant
                test_process_blockstore(genesis_config, blockstore, opts, &Arc::default())
            }
            AccessType::Secondary => {
                let secondary_blockstore = Blockstore::open_with_options(
                    blockstore.ledger_path(),
                    BlockstoreOptions {
                        access_type,
                        ..BlockstoreOptions::default()
                    },
                )
                .expect("Unable to open access to blockstore");
                test_process_blockstore(
                    genesis_config,
                    &secondary_blockstore,
                    opts,
                    &Arc::default(),
                )
            }
        }
    }

    #[test]
    fn test_process_blockstore_with_missing_hashes() {
        do_test_process_blockstore_with_missing_hashes(AccessType::Primary);
    }

    #[test]
    fn test_process_blockstore_with_missing_hashes_secondary_access() {
        do_test_process_blockstore_with_missing_hashes(AccessType::Secondary);
    }

    // Intentionally make slot 1 faulty and ensure that processing sees it as dead
    fn do_test_process_blockstore_with_missing_hashes(blockstore_access_type: AccessType) {
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

        let (bank_forks, ..) = test_process_blockstore_with_custom_options(
            &genesis_config,
            &blockstore,
            &ProcessOptions {
                poh_verify: true,
                ..ProcessOptions::default()
            },
            blockstore_access_type.clone(),
        );
        assert_eq!(frozen_bank_slots(&bank_forks.read().unwrap()), vec![0]);

        let dead_slots: Vec<Slot> = blockstore.dead_slots_iterator(0).unwrap().collect();
        match blockstore_access_type {
            // Secondary access is immutable so even though a dead slot
            // will be identified, it won't actually be marked dead.
            AccessType::Secondary => {
                assert_eq!(dead_slots.len(), 0);
            }
            AccessType::Primary | AccessType::PrimaryForMaintenance => {
                assert_eq!(&dead_slots, &[1]);
            }
        }
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
            &ProcessOptions {
                poh_verify: true,
                ..ProcessOptions::default()
            },
            &Arc::default(),
        );
        assert_eq!(frozen_bank_slots(&bank_forks.read().unwrap()), vec![0]);

        // Write slot 2 fully
        let _last_slot2_entry_hash =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 2, 0, blockhash);

        let (bank_forks, ..) = test_process_blockstore(
            &genesis_config,
            &blockstore,
            &ProcessOptions {
                poh_verify: true,
                ..ProcessOptions::default()
            },
            &Arc::default(),
        );

        // One valid fork, one bad fork.  process_blockstore() should only return the valid fork
        assert_eq!(frozen_bank_slots(&bank_forks.read().unwrap()), vec![0, 2]);
        assert_eq!(bank_forks.read().unwrap().working_bank().slot(), 2);
        assert_eq!(bank_forks.read().unwrap().root(), 0);
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
        let (bank_forks, ..) =
            test_process_blockstore(&genesis_config, &blockstore, &opts, &Arc::default());
        assert_eq!(frozen_bank_slots(&bank_forks.read().unwrap()), vec![0]);
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
        let (bank_forks, ..) =
            test_process_blockstore(&genesis_config, &blockstore, &opts, &Arc::default());

        assert_eq!(frozen_bank_slots(&bank_forks.read().unwrap()), vec![0]); // slot 1 isn't "full", we stop at slot zero

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
        let (bank_forks, ..) =
            test_process_blockstore(&genesis_config, &blockstore, &opts, &Arc::default());

        // slot 1 isn't "full", we stop at slot zero
        assert_eq!(frozen_bank_slots(&bank_forks.read().unwrap()), vec![0, 3]);
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
        let (bank_forks, ..) =
            test_process_blockstore(&genesis_config, &blockstore, &opts, &Arc::default());
        let bank_forks = bank_forks.read().unwrap();

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
        let (bank_forks, ..) =
            test_process_blockstore(&genesis_config, &blockstore, &opts, &Arc::default());
        let bank_forks = bank_forks.read().unwrap();

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

        let (bank_forks, ..) = test_process_blockstore(
            &genesis_config,
            &blockstore,
            &ProcessOptions::default(),
            &Arc::default(),
        );
        let bank_forks = bank_forks.read().unwrap();

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

        let (bank_forks, ..) = test_process_blockstore(
            &genesis_config,
            &blockstore,
            &ProcessOptions::default(),
            &Arc::default(),
        );
        let bank_forks = bank_forks.read().unwrap();

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
        let (bank_forks, ..) = test_process_blockstore(
            &genesis_config,
            &blockstore,
            &ProcessOptions::default(),
            &Arc::default(),
        );
        let bank_forks = bank_forks.read().unwrap();

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
        let (bank_forks, ..) =
            test_process_blockstore(&genesis_config, &blockstore, &opts, &Arc::default());
        let bank_forks = bank_forks.read().unwrap();

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
        let (bank_forks, ..) =
            test_process_blockstore(&genesis_config, &blockstore, &opts, &Arc::default());
        let bank_forks = bank_forks.read().unwrap();

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
        let (bank_forks, ..) =
            test_process_blockstore(&genesis_config, &blockstore, &opts, &Arc::default());
        let bank_forks = bank_forks.read().unwrap();

        assert_eq!(frozen_bank_slots(&bank_forks), vec![0]);
        let bank = bank_forks[0].clone();
        assert_eq!(bank.tick_height(), 1);
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
            test_process_blockstore(&genesis_config, &blockstore, &opts, &Arc::default());
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
            entry_callback: Some(entry_callback),
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        test_process_blockstore(&genesis_config, &blockstore, &opts, &Arc::default());
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
            halt_at_slot: Some(0),
            accounts_db_test_hash_calculation: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, ..) =
            test_process_blockstore(&genesis_config, &blockstore, &opts, &Arc::default());
        let bank_forks = bank_forks.read().unwrap();

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
        let bank_forks = RwLock::new(bank_forks);
        process_blockstore_from_root(
            &blockstore,
            &bank_forks,
            &leader_schedule_cache,
            &opts,
            None,
            None,
            &AbsRequestSender::default(),
        )
        .unwrap();

        let bank_forks = bank_forks.read().unwrap();

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
            Arc::<RuntimeConfig>::default(),
            account_paths,
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
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
            false,
            &mut ExecuteTimings::default(),
            None,
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
        let mut expected_signatures = BTreeSet::new();
        let vote_txs: Vec<_> = validator_keypairs
            .iter()
            .enumerate()
            .map(|(i, validator_keypairs)| {
                let vote_tx = if i % 3 == 0 {
                    // These votes are correct
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
                };
                expected_signatures.insert(vote_tx.signatures[0]);
                vote_tx
            })
            .collect();
        let entry = next_entry(&bank_1_blockhash, 1, vote_txs);
        let (replay_vote_sender, replay_vote_receiver) = crossbeam_channel::unbounded();
        let _ =
            process_entries_for_tests(&bank1, vec![entry], true, None, Some(&replay_vote_sender));
        let signatures: BTreeSet<_> = replay_vote_receiver
            .try_iter()
            .map(|(.., signature)| signature)
            .collect();
        assert_eq!(signatures, expected_signatures);
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

    fn run_test_process_blockstore_with_supermajority_root(
        blockstore_root: Option<Slot>,
        blockstore_access_type: AccessType,
    ) {
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

        let (bank_forks, ..) = test_process_blockstore_with_custom_options(
            &genesis_config,
            &blockstore,
            &opts,
            blockstore_access_type.clone(),
        );
        let bank_forks = bank_forks.read().unwrap();

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

        let (bank_forks, ..) = test_process_blockstore_with_custom_options(
            &genesis_config,
            &blockstore,
            &opts,
            blockstore_access_type.clone(),
        );
        let bank_forks = bank_forks.read().unwrap();

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

        let (bank_forks, ..) = test_process_blockstore_with_custom_options(
            &genesis_config,
            &blockstore,
            &opts,
            blockstore_access_type,
        );
        let bank_forks = bank_forks.read().unwrap();

        assert_eq!(bank_forks.root(), really_expected_root_slot);
    }

    #[test]
    fn test_process_blockstore_with_supermajority_root_without_blockstore_root() {
        run_test_process_blockstore_with_supermajority_root(None, AccessType::Primary);
    }

    #[test]
    fn test_process_blockstore_with_supermajority_root_without_blockstore_root_secondary_access() {
        run_test_process_blockstore_with_supermajority_root(None, AccessType::Secondary);
    }

    #[test]
    fn test_process_blockstore_with_supermajority_root_with_blockstore_root() {
        run_test_process_blockstore_with_supermajority_root(Some(1), AccessType::Primary)
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn test_supermajority_root_from_vote_accounts() {
        let convert_to_vote_accounts = |roots_stakes: Vec<(Slot, u64)>| -> VoteAccountsHashMap {
            roots_stakes
                .into_iter()
                .map(|(root, stake)| {
                    let mut vote_state = VoteState::default();
                    vote_state.root_slot = Some(root);
                    let mut vote_account =
                        AccountSharedData::new(1, VoteState::size_of(), &solana_vote_program::id());
                    let versioned = VoteStateVersions::new_current(vote_state);
                    VoteState::serialize(&versioned, vote_account.data_as_mut_slice()).unwrap();
                    (
                        solana_sdk::pubkey::new_rand(),
                        (stake, VoteAccount::try_from(vote_account).unwrap()),
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

    fn confirm_slot_entries_for_tests(
        bank: &Arc<Bank>,
        slot_entries: Vec<Entry>,
        slot_full: bool,
        prev_entry_hash: Hash,
    ) -> result::Result<(), BlockstoreProcessorError> {
        confirm_slot_entries(
            bank,
            (slot_entries, 0, slot_full),
            &mut ConfirmationTiming::default(),
            &mut ConfirmationProgress::new(prev_entry_hash),
            false,
            None,
            None,
            None,
            &VerifyRecyclers::default(),
            None,
            &PrioritizationFeeCache::new(0u64),
        )
    }

    #[test]
    fn test_confirm_slot_entries_without_fix() {
        const HASHES_PER_TICK: u64 = 10;
        const TICKS_PER_SLOT: u64 = 2;

        let collector_id = Pubkey::new_unique();

        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10_000);
        genesis_config.poh_config.hashes_per_tick = Some(HASHES_PER_TICK);
        genesis_config.ticks_per_slot = TICKS_PER_SLOT;
        let genesis_hash = genesis_config.hash();

        let mut slot_0_bank = Bank::new_for_tests(&genesis_config);
        slot_0_bank.deactivate_feature(&feature_set::fix_recent_blockhashes::id());
        let slot_0_bank = Arc::new(slot_0_bank);
        assert_eq!(slot_0_bank.slot(), 0);
        assert_eq!(slot_0_bank.tick_height(), 0);
        assert_eq!(slot_0_bank.max_tick_height(), 2);
        assert_eq!(slot_0_bank.last_blockhash(), genesis_hash);
        assert_eq!(slot_0_bank.get_hash_age(&genesis_hash), Some(0));

        let slot_0_entries = entry::create_ticks(TICKS_PER_SLOT, HASHES_PER_TICK, genesis_hash);
        let slot_0_hash = slot_0_entries.last().unwrap().hash;
        confirm_slot_entries_for_tests(&slot_0_bank, slot_0_entries, true, genesis_hash).unwrap();
        assert_eq!(slot_0_bank.tick_height(), slot_0_bank.max_tick_height());
        assert_eq!(slot_0_bank.last_blockhash(), slot_0_hash);
        assert_eq!(slot_0_bank.get_hash_age(&genesis_hash), Some(1));
        assert_eq!(slot_0_bank.get_hash_age(&slot_0_hash), Some(0));

        let slot_2_bank = Arc::new(Bank::new_from_parent(&slot_0_bank, &collector_id, 2));
        assert_eq!(slot_2_bank.slot(), 2);
        assert_eq!(slot_2_bank.tick_height(), 2);
        assert_eq!(slot_2_bank.max_tick_height(), 6);
        assert_eq!(slot_2_bank.last_blockhash(), slot_0_hash);

        let slot_1_entries = entry::create_ticks(TICKS_PER_SLOT, HASHES_PER_TICK, slot_0_hash);
        let slot_1_hash = slot_1_entries.last().unwrap().hash;
        confirm_slot_entries_for_tests(&slot_2_bank, slot_1_entries, false, slot_0_hash).unwrap();
        assert_eq!(slot_2_bank.tick_height(), 4);
        assert_eq!(slot_2_bank.last_blockhash(), slot_1_hash);
        assert_eq!(slot_2_bank.get_hash_age(&genesis_hash), Some(2));
        assert_eq!(slot_2_bank.get_hash_age(&slot_0_hash), Some(1));
        assert_eq!(slot_2_bank.get_hash_age(&slot_1_hash), Some(0));

        // Check that slot 2 transactions can use any previous slot hash, including the
        // hash for slot 1 which is just ticks.
        let slot_2_entries = {
            let to_pubkey = Pubkey::new_unique();
            let mut prev_entry_hash = slot_1_hash;
            let mut remaining_entry_hashes = HASHES_PER_TICK;
            let mut entries: Vec<Entry> = [genesis_hash, slot_0_hash, slot_1_hash]
                .into_iter()
                .map(|recent_hash| {
                    let tx =
                        system_transaction::transfer(&mint_keypair, &to_pubkey, 1, recent_hash);
                    remaining_entry_hashes = remaining_entry_hashes.checked_sub(1).unwrap();
                    next_entry_mut(&mut prev_entry_hash, 1, vec![tx])
                })
                .collect();

            entries.push(next_entry_mut(
                &mut prev_entry_hash,
                remaining_entry_hashes,
                vec![],
            ));
            entries.push(next_entry_mut(
                &mut prev_entry_hash,
                HASHES_PER_TICK,
                vec![],
            ));
            entries
        };
        let slot_2_hash = slot_2_entries.last().unwrap().hash;
        confirm_slot_entries_for_tests(&slot_2_bank, slot_2_entries, true, slot_1_hash).unwrap();
        assert_eq!(slot_2_bank.tick_height(), slot_2_bank.max_tick_height());
        assert_eq!(slot_2_bank.last_blockhash(), slot_2_hash);
        assert_eq!(slot_2_bank.get_hash_age(&genesis_hash), Some(3));
        assert_eq!(slot_2_bank.get_hash_age(&slot_0_hash), Some(2));
        assert_eq!(slot_2_bank.get_hash_age(&slot_1_hash), Some(1));
        assert_eq!(slot_2_bank.get_hash_age(&slot_2_hash), Some(0));
    }

    #[test]
    fn test_confirm_slot_entries_progress_num_txs_indexes() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100 * LAMPORTS_PER_SOL);
        let genesis_hash = genesis_config.hash();
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let mut timing = ConfirmationTiming::default();
        let mut progress = ConfirmationProgress::new(genesis_hash);
        let amount = genesis_config.rent.minimum_balance(0);
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        let keypair4 = Keypair::new();
        bank.transfer(LAMPORTS_PER_SOL, &mint_keypair, &keypair1.pubkey())
            .unwrap();
        bank.transfer(LAMPORTS_PER_SOL, &mint_keypair, &keypair2.pubkey())
            .unwrap();

        let (transaction_status_sender, transaction_status_receiver) =
            crossbeam_channel::unbounded();
        let transaction_status_sender = TransactionStatusSender {
            sender: transaction_status_sender,
        };

        let blockhash = bank.last_blockhash();
        let tx1 = system_transaction::transfer(
            &keypair1,
            &keypair3.pubkey(),
            amount,
            bank.last_blockhash(),
        );
        let tx2 = system_transaction::transfer(
            &keypair2,
            &keypair4.pubkey(),
            amount,
            bank.last_blockhash(),
        );
        let entry = next_entry(&blockhash, 1, vec![tx1, tx2]);
        let new_hash = entry.hash;

        confirm_slot_entries(
            &bank,
            (vec![entry], 0, false),
            &mut timing,
            &mut progress,
            false,
            Some(&transaction_status_sender),
            None,
            None,
            &VerifyRecyclers::default(),
            None,
            &PrioritizationFeeCache::new(0u64),
        )
        .unwrap();
        assert_eq!(progress.num_txs, 2);
        let batch = transaction_status_receiver.recv().unwrap();
        if let TransactionStatusMessage::Batch(batch) = batch {
            assert_eq!(batch.transactions.len(), 2);
            assert_eq!(batch.transaction_indexes.len(), 2);
            // Assert contains instead of the actual vec due to randomize
            assert!(batch.transaction_indexes.contains(&0));
            assert!(batch.transaction_indexes.contains(&1));
        } else {
            panic!("batch should have been sent");
        }

        let tx1 = system_transaction::transfer(
            &keypair1,
            &keypair3.pubkey(),
            amount + 1,
            bank.last_blockhash(),
        );
        let tx2 = system_transaction::transfer(
            &keypair2,
            &keypair4.pubkey(),
            amount + 1,
            bank.last_blockhash(),
        );
        let tx3 = system_transaction::transfer(
            &mint_keypair,
            &Pubkey::new_unique(),
            amount,
            bank.last_blockhash(),
        );
        let entry = next_entry(&new_hash, 1, vec![tx1, tx2, tx3]);

        confirm_slot_entries(
            &bank,
            (vec![entry], 0, false),
            &mut timing,
            &mut progress,
            false,
            Some(&transaction_status_sender),
            None,
            None,
            &VerifyRecyclers::default(),
            None,
            &PrioritizationFeeCache::new(0u64),
        )
        .unwrap();
        assert_eq!(progress.num_txs, 5);
        let batch = transaction_status_receiver.recv().unwrap();
        if let TransactionStatusMessage::Batch(batch) = batch {
            assert_eq!(batch.transactions.len(), 3);
            assert_eq!(batch.transaction_indexes.len(), 3);
            // Assert contains instead of the actual vec due to randomize
            assert!(batch.transaction_indexes.contains(&2));
            assert!(batch.transaction_indexes.contains(&3));
            assert!(batch.transaction_indexes.contains(&4));
        } else {
            panic!("batch should have been sent");
        }
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
        let keypair3 = Keypair::new();
        let pubkey3 = solana_sdk::pubkey::new_rand();

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
            SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
                &keypair3,
                &pubkey3,
                1,
                genesis_config.hash(),
            )),
        ];

        let batch = bank.prepare_sanitized_batch(&txs);
        assert!(batch.needs_unlock());
        let transaction_indexes = vec![42, 43, 44];

        let batch2 = rebatch_transactions(
            batch.lock_results(),
            &bank,
            batch.sanitized_transactions(),
            0,
            0,
            &transaction_indexes,
        );
        assert!(batch.needs_unlock());
        assert!(!batch2.batch.needs_unlock());
        assert_eq!(batch2.transaction_indexes, vec![42]);

        let batch3 = rebatch_transactions(
            batch.lock_results(),
            &bank,
            batch.sanitized_transactions(),
            1,
            2,
            &transaction_indexes,
        );
        assert!(!batch3.batch.needs_unlock());
        assert_eq!(batch3.transaction_indexes, vec![43, 44]);
    }

    #[test]
    fn test_confirm_slot_entries_with_fix() {
        const HASHES_PER_TICK: u64 = 10;
        const TICKS_PER_SLOT: u64 = 2;

        let collector_id = Pubkey::new_unique();

        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10_000);
        genesis_config.poh_config.hashes_per_tick = Some(HASHES_PER_TICK);
        genesis_config.ticks_per_slot = TICKS_PER_SLOT;
        let genesis_hash = genesis_config.hash();

        let slot_0_bank = Arc::new(Bank::new_for_tests(&genesis_config));
        assert_eq!(slot_0_bank.slot(), 0);
        assert_eq!(slot_0_bank.tick_height(), 0);
        assert_eq!(slot_0_bank.max_tick_height(), 2);
        assert_eq!(slot_0_bank.last_blockhash(), genesis_hash);
        assert_eq!(slot_0_bank.get_hash_age(&genesis_hash), Some(0));

        let slot_0_entries = entry::create_ticks(TICKS_PER_SLOT, HASHES_PER_TICK, genesis_hash);
        let slot_0_hash = slot_0_entries.last().unwrap().hash;
        confirm_slot_entries_for_tests(&slot_0_bank, slot_0_entries, true, genesis_hash).unwrap();
        assert_eq!(slot_0_bank.tick_height(), slot_0_bank.max_tick_height());
        assert_eq!(slot_0_bank.last_blockhash(), slot_0_hash);
        assert_eq!(slot_0_bank.get_hash_age(&genesis_hash), Some(1));
        assert_eq!(slot_0_bank.get_hash_age(&slot_0_hash), Some(0));

        let slot_2_bank = Arc::new(Bank::new_from_parent(&slot_0_bank, &collector_id, 2));
        assert_eq!(slot_2_bank.slot(), 2);
        assert_eq!(slot_2_bank.tick_height(), 2);
        assert_eq!(slot_2_bank.max_tick_height(), 6);
        assert_eq!(slot_2_bank.last_blockhash(), slot_0_hash);

        let slot_1_entries = entry::create_ticks(TICKS_PER_SLOT, HASHES_PER_TICK, slot_0_hash);
        let slot_1_hash = slot_1_entries.last().unwrap().hash;
        confirm_slot_entries_for_tests(&slot_2_bank, slot_1_entries, false, slot_0_hash).unwrap();
        assert_eq!(slot_2_bank.tick_height(), 4);
        assert_eq!(slot_2_bank.last_blockhash(), slot_0_hash);
        assert_eq!(slot_2_bank.get_hash_age(&genesis_hash), Some(1));
        assert_eq!(slot_2_bank.get_hash_age(&slot_0_hash), Some(0));

        struct TestCase {
            recent_blockhash: Hash,
            expected_result: result::Result<(), BlockstoreProcessorError>,
        }

        let test_cases = [
            TestCase {
                recent_blockhash: slot_1_hash,
                expected_result: Err(BlockstoreProcessorError::InvalidTransaction(
                    TransactionError::BlockhashNotFound,
                )),
            },
            TestCase {
                recent_blockhash: slot_0_hash,
                expected_result: Ok(()),
            },
        ];

        // Check that slot 2 transactions can only use hashes for completed blocks.
        for TestCase {
            recent_blockhash,
            expected_result,
        } in test_cases
        {
            let slot_2_entries = {
                let to_pubkey = Pubkey::new_unique();
                let mut prev_entry_hash = slot_1_hash;
                let mut remaining_entry_hashes = HASHES_PER_TICK;

                let tx =
                    system_transaction::transfer(&mint_keypair, &to_pubkey, 1, recent_blockhash);
                remaining_entry_hashes = remaining_entry_hashes.checked_sub(1).unwrap();
                let mut entries = vec![next_entry_mut(&mut prev_entry_hash, 1, vec![tx])];

                entries.push(next_entry_mut(
                    &mut prev_entry_hash,
                    remaining_entry_hashes,
                    vec![],
                ));
                entries.push(next_entry_mut(
                    &mut prev_entry_hash,
                    HASHES_PER_TICK,
                    vec![],
                ));

                entries
            };

            let slot_2_hash = slot_2_entries.last().unwrap().hash;
            let result =
                confirm_slot_entries_for_tests(&slot_2_bank, slot_2_entries, true, slot_1_hash);
            match (result, expected_result) {
                (Ok(()), Ok(())) => {
                    assert_eq!(slot_2_bank.tick_height(), slot_2_bank.max_tick_height());
                    assert_eq!(slot_2_bank.last_blockhash(), slot_2_hash);
                    assert_eq!(slot_2_bank.get_hash_age(&genesis_hash), Some(2));
                    assert_eq!(slot_2_bank.get_hash_age(&slot_0_hash), Some(1));
                    assert_eq!(slot_2_bank.get_hash_age(&slot_2_hash), Some(0));
                }
                (
                    Err(BlockstoreProcessorError::InvalidTransaction(err)),
                    Err(BlockstoreProcessorError::InvalidTransaction(expected_err)),
                ) => {
                    assert_eq!(err, expected_err);
                }
                (result, expected_result) => {
                    panic!("actual result {result:?} != expected result {expected_result:?}");
                }
            }
        }
    }
}

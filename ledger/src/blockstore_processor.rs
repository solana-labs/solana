use crate::{
    bank_forks::BankForks,
    block_error::BlockError,
    blockstore::Blockstore,
    blockstore_db::BlockstoreError,
    blockstore_meta::SlotMeta,
    entry::{create_ticks, Entry, EntrySlice, EntryVerificationStatus, VerifyRecyclers},
    leader_schedule_cache::LeaderScheduleCache,
};
use crossbeam_channel::Sender;
use itertools::Itertools;
use log::*;
use rand::{seq::SliceRandom, thread_rng};
use rayon::{prelude::*, ThreadPool};
use solana_measure::{measure::Measure, thread_mem_usage};
use solana_metrics::{datapoint_error, inc_new_counter_debug};
use solana_rayon_threadlimit::get_thread_count;
use solana_runtime::{
    bank::{Bank, TransactionBalancesSet, TransactionProcessResult, TransactionResults},
    transaction_batch::TransactionBatch,
};
use solana_sdk::{
    clock::{Slot, MAX_PROCESSING_AGE},
    genesis_config::GenesisConfig,
    hash::Hash,
    pubkey::Pubkey,
    signature::Keypair,
    timing::duration_as_ms,
    transaction::{Result, Transaction, TransactionError},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    result,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;

pub type BlockstoreProcessorResult =
    result::Result<(BankForks, LeaderScheduleCache), BlockstoreProcessorError>;

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

fn execute_batch(
    batch: &TransactionBatch,
    bank: &Arc<Bank>,
    transaction_status_sender: Option<TransactionStatusSender>,
) -> Result<()> {
    let (
        TransactionResults {
            fee_collection_results,
            processing_results,
        },
        balances,
    ) = batch.bank().load_execute_and_commit_transactions(
        batch,
        MAX_PROCESSING_AGE,
        transaction_status_sender.is_some(),
    );

    if let Some(sender) = transaction_status_sender {
        send_transaction_status_batch(
            bank.clone(),
            batch.transactions(),
            processing_results,
            balances,
            sender,
        );
    }

    let mut first_err = None;
    for (result, transaction) in fee_collection_results.iter().zip(batch.transactions()) {
        if let Err(ref err) = result {
            if first_err.is_none() {
                first_err = Some(result.clone());
            }
            warn!(
                "Unexpected validator error: {:?}, transaction: {:?}",
                err, transaction
            );
            datapoint_error!(
                "validator_process_entry_error",
                (
                    "error",
                    format!("error: {:?}, transaction: {:?}", err, transaction),
                    String
                )
            );
        }
    }
    first_err.unwrap_or(Ok(()))
}

fn execute_batches(
    bank: &Arc<Bank>,
    batches: &[TransactionBatch],
    entry_callback: Option<&ProcessCallback>,
    transaction_status_sender: Option<TransactionStatusSender>,
) -> Result<()> {
    inc_new_counter_debug!("bank-par_execute_entries-count", batches.len());
    let results: Vec<Result<()>> = PAR_THREAD_POOL.with(|thread_pool| {
        thread_pool.borrow().install(|| {
            batches
                .into_par_iter()
                .map_with(transaction_status_sender, |sender, batch| {
                    let result = execute_batch(batch, bank, sender.clone());
                    if let Some(entry_callback) = entry_callback {
                        entry_callback(bank);
                    }
                    result
                })
                .collect()
        })
    });

    first_err(&results)
}

/// Process an ordered list of entries in parallel
/// 1. In order lock accounts for each entry while the lock succeeds, up to a Tick entry
/// 2. Process the locked group in parallel
/// 3. Register the `Tick` if it's available
/// 4. Update the leader scheduler, goto 1
pub fn process_entries(
    bank: &Arc<Bank>,
    entries: &[Entry],
    randomize: bool,
    transaction_status_sender: Option<TransactionStatusSender>,
) -> Result<()> {
    process_entries_with_callback(bank, entries, randomize, None, transaction_status_sender)
}

fn process_entries_with_callback(
    bank: &Arc<Bank>,
    entries: &[Entry],
    randomize: bool,
    entry_callback: Option<&ProcessCallback>,
    transaction_status_sender: Option<TransactionStatusSender>,
) -> Result<()> {
    // accumulator for entries that can be processed in parallel
    let mut batches = vec![];
    let mut tick_hashes = vec![];
    for entry in entries {
        if entry.is_tick() {
            // If it's a tick, save it for later
            tick_hashes.push(entry.hash);
            if bank.is_block_boundary(bank.tick_height() + tick_hashes.len() as u64) {
                // If it's a tick that will cause a new blockhash to be created,
                // execute the group and register the tick
                execute_batches(
                    bank,
                    &batches,
                    entry_callback,
                    transaction_status_sender.clone(),
                )?;
                batches.clear();
                for hash in &tick_hashes {
                    bank.register_tick(hash);
                }
                tick_hashes.clear();
            }
            continue;
        }
        // else loop on processing the entry
        loop {
            let iteration_order = if randomize {
                let mut iteration_order: Vec<usize> = (0..entry.transactions.len()).collect();
                iteration_order.shuffle(&mut thread_rng());
                Some(iteration_order)
            } else {
                None
            };

            // try to lock the accounts
            let batch = bank.prepare_batch(&entry.transactions, iteration_order);

            let first_lock_err = first_err(batch.lock_results());

            // if locking worked
            if first_lock_err.is_ok() {
                batches.push(batch);
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
                            "Lock accounts error, entry conflicts with itself, txs: {:?}",
                            entry.transactions
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
                    transaction_status_sender.clone(),
                )?;
                batches.clear();
            }
        }
    }
    execute_batches(bank, &batches, entry_callback, transaction_status_sender)?;
    for hash in tick_hashes {
        bank.register_tick(&hash);
    }
    Ok(())
}

#[derive(Error, Debug)]
pub enum BlockstoreProcessorError {
    #[error("failed to load entries")]
    FailedToLoadEntries(#[from] BlockstoreError),

    #[error("failed to load meta")]
    FailedToLoadMeta,

    #[error("invalid block")]
    InvalidBlock(#[from] BlockError),

    #[error("invalid transaction")]
    InvalidTransaction(#[from] TransactionError),

    #[error("no valid forks found")]
    NoValidForksFound,

    #[error("invalid hard fork")]
    InvalidHardFork(Slot),
}

/// Callback for accessing bank state while processing the blockstore
pub type ProcessCallback = Arc<dyn Fn(&Bank) -> () + Sync + Send>;

#[derive(Default, Clone)]
pub struct ProcessOptions {
    pub poh_verify: bool,
    pub full_leader_cache: bool,
    pub dev_halt_at_slot: Option<Slot>,
    pub entry_callback: Option<ProcessCallback>,
    pub override_num_threads: Option<usize>,
    pub new_hard_forks: Option<Vec<Slot>>,
    pub frozen_accounts: Vec<Pubkey>,
}

pub fn process_blockstore(
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    account_paths: Vec<PathBuf>,
    opts: ProcessOptions,
) -> BlockstoreProcessorResult {
    if let Some(num_threads) = opts.override_num_threads {
        PAR_THREAD_POOL.with(|pool| {
            *pool.borrow_mut() = rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .unwrap()
        });
    }

    // Setup bank for slot 0
    let bank0 = Arc::new(Bank::new_with_paths(
        &genesis_config,
        account_paths,
        &opts.frozen_accounts,
    ));
    info!("processing ledger for slot 0...");
    let recyclers = VerifyRecyclers::default();
    process_bank_0(&bank0, blockstore, &opts, &recyclers)?;
    process_blockstore_from_root(genesis_config, blockstore, bank0, &opts, &recyclers)
}

// Process blockstore from a known root bank
pub fn process_blockstore_from_root(
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    bank: Arc<Bank>,
    opts: &ProcessOptions,
    recyclers: &VerifyRecyclers,
) -> BlockstoreProcessorResult {
    info!("processing ledger from slot {}...", bank.slot());
    let allocated = thread_mem_usage::Allocatedp::default();
    let initial_allocation = allocated.get();

    // Starting slot must be a root, and thus has no parents
    assert!(bank.parent().is_none());
    let start_slot = bank.slot();
    let now = Instant::now();
    let mut root = start_slot;

    bank.set_entered_epoch_callback(solana_genesis_programs::get_entered_epoch_callback(
        genesis_config.operating_mode,
    ));

    if let Some(ref new_hard_forks) = opts.new_hard_forks {
        let hard_forks = bank.hard_forks();

        for hard_fork_slot in new_hard_forks.iter() {
            // Ensure the user isn't trying to add new hard forks for a slot that's earlier than the current
            // root slot.  Doing so won't cause any effect so emit an error
            if *hard_fork_slot <= start_slot {
                error!(
                    "Unable to add new hard fork at {}, it must be greater than slot {}",
                    hard_fork_slot, start_slot
                );
                return Err(BlockstoreProcessorError::InvalidHardFork(*hard_fork_slot));
            }
            hard_forks.write().unwrap().register(*hard_fork_slot);
        }
    }

    // ensure start_slot is rooted for correct replay
    if blockstore.is_primary_access() {
        blockstore
            .set_roots(&[start_slot])
            .expect("Couldn't set root slot on startup");
    } else if !blockstore.is_root(start_slot) {
        panic!("starting slot isn't root and can't update due to being secondary blockstore access: {}", start_slot);
    }

    if let Ok(metas) = blockstore.slot_meta_iterator(start_slot) {
        if let Some((slot, _meta)) = metas.last() {
            info!("ledger holds data through slot {}", slot);
        }
    }

    // Iterate and replay slots from blockstore starting from `start_slot`
    let (initial_forks, leader_schedule_cache) = {
        if let Some(meta) = blockstore
            .meta(start_slot)
            .unwrap_or_else(|_| panic!("Failed to get meta for slot {}", start_slot))
        {
            let epoch_schedule = bank.epoch_schedule();
            let mut leader_schedule_cache = LeaderScheduleCache::new(*epoch_schedule, &bank);
            if opts.full_leader_cache {
                leader_schedule_cache.set_max_schedules(std::usize::MAX);
            }
            let initial_forks = load_frozen_forks(
                &bank,
                &meta,
                blockstore,
                &mut leader_schedule_cache,
                &mut root,
                opts,
                recyclers,
            )?;
            (initial_forks, leader_schedule_cache)
        } else {
            // If there's no meta for the input `start_slot`, then we started from a snapshot
            // and there's no point in processing the rest of blockstore and implies blockstore
            // should be empty past this point.
            let leader_schedule_cache = LeaderScheduleCache::new_from_bank(&bank);
            (vec![bank], leader_schedule_cache)
        }
    };
    if initial_forks.is_empty() {
        return Err(BlockstoreProcessorError::NoValidForksFound);
    }
    let bank_forks = BankForks::new_from_banks(&initial_forks, root);

    info!(
        "ledger processed in {}ms. {} MB allocated. root={}, {} fork{} at {}, with {} frozen bank{}",
        duration_as_ms(&now.elapsed()),
        allocated.since(initial_allocation) / 1_000_000,
        bank_forks.root(),
        initial_forks.len(),
        if initial_forks.len() > 1 { "s" } else { "" },
        initial_forks
            .iter()
            .map(|b| b.slot().to_string())
            .join(", "),
        bank_forks.frozen_banks().len(),
        if bank_forks.frozen_banks().len() > 1 {
            "s"
        } else {
            ""
        },
    );
    assert!(bank_forks.active_banks().is_empty());

    Ok((bank_forks, leader_schedule_cache))
}

/// Verify that a segment of entries has the correct number of ticks and hashes
pub fn verify_ticks(
    bank: &Arc<Bank>,
    entries: &[Entry],
    slot_full: bool,
    tick_hash_count: &mut u64,
) -> std::result::Result<(), BlockError> {
    let next_bank_tick_height = bank.tick_height() + entries.tick_count();
    let max_bank_tick_height = bank.max_tick_height();
    if next_bank_tick_height > max_bank_tick_height {
        warn!("Too many entry ticks found in slot: {}", bank.slot());
        return Err(BlockError::InvalidTickCount);
    }

    if next_bank_tick_height < max_bank_tick_height && slot_full {
        warn!("Too few entry ticks found in slot: {}", bank.slot());
        return Err(BlockError::InvalidTickCount);
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
) -> result::Result<(), BlockstoreProcessorError> {
    let mut timing = ConfirmationTiming::default();
    let skip_verification = !opts.poh_verify;
    confirm_slot(
        blockstore,
        bank,
        &mut timing,
        progress,
        skip_verification,
        None,
        opts.entry_callback.as_ref(),
        recyclers,
    )?;

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
    pub verify_elapsed: u64,
    pub fetch_elapsed: u64,
    pub fetch_fail_elapsed: u64,
}

impl Default for ConfirmationTiming {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            replay_elapsed: 0,
            verify_elapsed: 0,
            fetch_elapsed: 0,
            fetch_fail_elapsed: 0,
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

pub fn confirm_slot(
    blockstore: &Blockstore,
    bank: &Arc<Bank>,
    timing: &mut ConfirmationTiming,
    progress: &mut ConfirmationProgress,
    skip_verification: bool,
    transaction_status_sender: Option<TransactionStatusSender>,
    entry_callback: Option<&ProcessCallback>,
    recyclers: &VerifyRecyclers,
) -> result::Result<(), BlockstoreProcessorError> {
    let slot = bank.slot();

    let (entries, num_shreds, slot_full) = {
        let mut load_elapsed = Measure::start("load_elapsed");
        let load_result = blockstore
            .get_slot_entries_with_shred_info(slot, progress.num_shreds, false)
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

    let mut replay_elapsed = Measure::start("replay_elapsed");
    let process_result = process_entries_with_callback(
        bank,
        &entries,
        true,
        entry_callback,
        transaction_status_sender,
    )
    .map_err(BlockstoreProcessorError::from);
    replay_elapsed.stop();
    timing.replay_elapsed += replay_elapsed.as_us();

    if let Some(mut verifier) = verifier {
        if !verifier.finish_verify(&entries) {
            warn!("Ledger proof of history failed at slot: {}", bank.slot());
            return Err(BlockError::InvalidEntryHash.into());
        }
        timing.verify_elapsed += verifier.duration_ms();
    }

    process_result?;

    progress.num_shreds += num_shreds;
    progress.num_entries += num_entries;
    progress.num_txs += num_txs;
    if let Some(last_entry) = entries.last() {
        progress.last_entry = last_entry.hash;
    }

    Ok(())
}

// Special handling required for processing the entries in slot 0
fn process_bank_0(
    bank0: &Arc<Bank>,
    blockstore: &Blockstore,
    opts: &ProcessOptions,
    recyclers: &VerifyRecyclers,
) -> result::Result<(), BlockstoreProcessorError> {
    assert_eq!(bank0.slot(), 0);
    let mut progress = ConfirmationProgress::new(bank0.last_blockhash());
    confirm_full_slot(blockstore, bank0, opts, recyclers, &mut progress)
        .expect("processing for bank 0 must succeed");
    bank0.freeze();
    Ok(())
}

// Given a bank, add its children to the pending slots queue if those children slots are
// complete
fn process_next_slots(
    bank: &Arc<Bank>,
    meta: &SlotMeta,
    blockstore: &Blockstore,
    leader_schedule_cache: &LeaderScheduleCache,
    pending_slots: &mut Vec<(SlotMeta, Arc<Bank>, Hash)>,
    initial_forks: &mut HashMap<Slot, Arc<Bank>>,
) -> result::Result<(), BlockstoreProcessorError> {
    if let Some(parent) = bank.parent() {
        initial_forks.remove(&parent.slot());
    }
    initial_forks.insert(bank.slot(), bank.clone());

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
            let allocated = thread_mem_usage::Allocatedp::default();
            let initial_allocation = allocated.get();

            let next_bank = Arc::new(Bank::new_from_parent(
                &bank,
                &leader_schedule_cache
                    .slot_leader_at(*next_slot, Some(&bank))
                    .unwrap(),
                *next_slot,
            ));
            trace!(
                "New bank for slot {}, parent slot is {}. {} bytes allocated",
                next_slot,
                bank.slot(),
                allocated.since(initial_allocation)
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
fn load_frozen_forks(
    root_bank: &Arc<Bank>,
    root_meta: &SlotMeta,
    blockstore: &Blockstore,
    leader_schedule_cache: &mut LeaderScheduleCache,
    root: &mut Slot,
    opts: &ProcessOptions,
    recyclers: &VerifyRecyclers,
) -> result::Result<Vec<Arc<Bank>>, BlockstoreProcessorError> {
    let mut initial_forks = HashMap::new();
    let mut last_status_report = Instant::now();
    let mut pending_slots = vec![];
    let mut last_root_slot = root_bank.slot();
    let mut slots_elapsed = 0;
    let mut txs = 0;
    process_next_slots(
        root_bank,
        root_meta,
        blockstore,
        leader_schedule_cache,
        &mut pending_slots,
        &mut initial_forks,
    )?;

    let dev_halt_at_slot = opts.dev_halt_at_slot.unwrap_or(std::u64::MAX);
    while !pending_slots.is_empty() {
        let (meta, bank, last_entry_hash) = pending_slots.pop().unwrap();
        let slot = bank.slot();
        if last_status_report.elapsed() > Duration::from_secs(2) {
            let secs = last_status_report.elapsed().as_secs() as f32;
            info!(
                "processing ledger: slot={}, last root slot={} slots={} slots/s={:?} txs/s={}",
                slot,
                last_root_slot,
                slots_elapsed,
                slots_elapsed as f32 / secs,
                txs as f32 / secs,
            );
            last_status_report = Instant::now();
            slots_elapsed = 0;
            txs = 0;
        }

        let allocated = thread_mem_usage::Allocatedp::default();
        let initial_allocation = allocated.get();

        let mut progress = ConfirmationProgress::new(last_entry_hash);
        if process_single_slot(blockstore, &bank, opts, recyclers, &mut progress).is_err() {
            continue;
        }
        txs += progress.num_txs;

        if blockstore.is_root(slot) {
            *root = slot;
            leader_schedule_cache.set_root(&bank);
            bank.squash();
            pending_slots.clear();
            initial_forks.clear();
            last_root_slot = slot;
        }
        slots_elapsed += 1;

        trace!(
            "Bank for {}slot {} is complete. {} bytes allocated",
            if last_root_slot == slot { "root " } else { "" },
            slot,
            allocated.since(initial_allocation)
        );

        process_next_slots(
            &bank,
            &meta,
            blockstore,
            leader_schedule_cache,
            &mut pending_slots,
            &mut initial_forks,
        )?;

        if slot >= dev_halt_at_slot {
            break;
        }
    }

    Ok(initial_forks.values().cloned().collect::<Vec<_>>())
}

// Processes and replays the contents of a single slot, returns Error
// if failed to play the slot
fn process_single_slot(
    blockstore: &Blockstore,
    bank: &Arc<Bank>,
    opts: &ProcessOptions,
    recyclers: &VerifyRecyclers,
    progress: &mut ConfirmationProgress,
) -> result::Result<(), BlockstoreProcessorError> {
    // Mark corrupt slots as dead so validators don't replay this slot and
    // see DuplicateSignature errors later in ReplayStage
    confirm_full_slot(blockstore, bank, opts, recyclers, progress).map_err(|err| {
        let slot = bank.slot();
        warn!("slot {} failed to verify: {}", slot, err);
        if blockstore.is_primary_access() {
            blockstore
                .set_dead_slot(slot)
                .expect("Failed to mark slot as dead in blockstore");
        } else if !blockstore.is_dead(slot) {
            panic!("Failed slot isn't dead and can't update due to being secondary blockstore access: {}", slot);
        }
        err
    })?;

    bank.freeze(); // all banks handled by this routine are created from complete slots
    Ok(())
}

pub struct TransactionStatusBatch {
    pub bank: Arc<Bank>,
    pub transactions: Vec<Transaction>,
    pub statuses: Vec<TransactionProcessResult>,
    pub balances: TransactionBalancesSet,
}
pub type TransactionStatusSender = Sender<TransactionStatusBatch>;

pub fn send_transaction_status_batch(
    bank: Arc<Bank>,
    transactions: &[Transaction],
    statuses: Vec<TransactionProcessResult>,
    balances: TransactionBalancesSet,
    transaction_status_sender: TransactionStatusSender,
) {
    let slot = bank.slot();
    if let Err(e) = transaction_status_sender.send(TransactionStatusBatch {
        bank,
        transactions: transactions.to_vec(),
        statuses,
        balances,
    }) {
        trace!(
            "Slot {} transaction_status send batch failed: {:?}",
            slot,
            e
        );
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
    use super::*;
    use crate::{
        entry::{create_ticks, next_entry, next_entry_mut},
        genesis_utils::{
            create_genesis_config, create_genesis_config_with_leader, GenesisConfigInfo,
        },
    };
    use matches::assert_matches;
    use rand::{thread_rng, Rng};
    use solana_sdk::account::Account;
    use solana_sdk::{
        epoch_schedule::EpochSchedule,
        hash::Hash,
        instruction::InstructionError,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_transaction,
        transaction::{Transaction, TransactionError},
    };
    use std::sync::RwLock;

    #[test]
    fn test_process_blockstore_with_missing_hashes() {
        solana_logger::setup();

        let hashes_per_tick = 2;
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config(10_000);
        genesis_config.poh_config.hashes_per_tick = Some(hashes_per_tick);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
        let blockstore =
            Blockstore::open(&ledger_path).expect("Expected to successfully open database ledger");

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

        let (bank_forks, _leader_schedule) = process_blockstore(
            &genesis_config,
            &blockstore,
            Vec::new(),
            ProcessOptions {
                poh_verify: true,
                ..ProcessOptions::default()
            },
        )
        .unwrap();
        assert_eq!(frozen_bank_slots(&bank_forks), vec![0]);
    }

    #[test]
    fn test_process_blockstore_with_invalid_slot_tick_count() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
        let blockstore = Blockstore::open(&ledger_path).unwrap();

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
        let (bank_forks, _leader_schedule) = process_blockstore(
            &genesis_config,
            &blockstore,
            Vec::new(),
            ProcessOptions {
                poh_verify: true,
                ..ProcessOptions::default()
            },
        )
        .unwrap();
        assert_eq!(frozen_bank_slots(&bank_forks), vec![0]);

        // Write slot 2 fully
        let _last_slot2_entry_hash =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 2, 0, blockhash);

        let (bank_forks, _leader_schedule) = process_blockstore(
            &genesis_config,
            &blockstore,
            Vec::new(),
            ProcessOptions {
                poh_verify: true,
                ..ProcessOptions::default()
            },
        )
        .unwrap();

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

        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
        let blockstore = Blockstore::open(&ledger_path).unwrap();

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
            ..ProcessOptions::default()
        };
        let (bank_forks, _leader_schedule) =
            process_blockstore(&genesis_config, &blockstore, Vec::new(), opts).unwrap();
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
        let (ledger_path, mut blockhash) = create_new_tmp_ledger!(&genesis_config);
        debug!("ledger_path: {:?}", ledger_path);

        let blockstore =
            Blockstore::open(&ledger_path).expect("Expected to successfully open database ledger");

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
            ..ProcessOptions::default()
        };
        let (bank_forks, _leader_schedule) =
            process_blockstore(&genesis_config, &blockstore, Vec::new(), opts).unwrap();

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
            ..ProcessOptions::default()
        };
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 3, 0, blockhash);
        // Slot 0 should not show up in the ending bank_forks_info
        let (bank_forks, _leader_schedule) =
            process_blockstore(&genesis_config, &blockstore, Vec::new(), opts).unwrap();

        // slot 1 isn't "full", we stop at slot zero
        assert_eq!(frozen_bank_slots(&bank_forks), vec![0, 3]);
    }

    #[test]
    fn test_process_blockstore_with_two_forks_and_squash() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
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
        let blockstore =
            Blockstore::open(&ledger_path).expect("Expected to successfully open database ledger");

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

        blockstore.set_roots(&[0, 1, 4]).unwrap();

        let opts = ProcessOptions {
            poh_verify: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, _leader_schedule) =
            process_blockstore(&genesis_config, &blockstore, Vec::new(), opts).unwrap();

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
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
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
        let blockstore =
            Blockstore::open(&ledger_path).expect("Expected to successfully open database ledger");

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

        blockstore.set_roots(&[0, 1]).unwrap();

        let opts = ProcessOptions {
            poh_verify: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, _leader_schedule) =
            process_blockstore(&genesis_config, &blockstore, Vec::new(), opts).unwrap();

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
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
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
        let blockstore = Blockstore::open(&ledger_path).unwrap();
        let slot1_blockhash =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 1, 0, blockhash);
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 2, 1, slot1_blockhash);
        blockstore.set_dead_slot(2).unwrap();
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 3, 1, slot1_blockhash);

        let (bank_forks, _leader_schedule) = process_blockstore(
            &genesis_config,
            &blockstore,
            Vec::new(),
            ProcessOptions::default(),
        )
        .unwrap();

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
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
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
        let blockstore = Blockstore::open(&ledger_path).unwrap();
        let slot1_blockhash =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 1, 0, blockhash);
        let slot2_blockhash =
            fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 2, 1, slot1_blockhash);
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 4, 2, slot2_blockhash);
        blockstore.set_dead_slot(4).unwrap();
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 3, 1, slot1_blockhash);

        let (bank_forks, _leader_schedule) = process_blockstore(
            &genesis_config,
            &blockstore,
            Vec::new(),
            ProcessOptions::default(),
        )
        .unwrap();

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
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
        debug!("ledger_path: {:?}", ledger_path);

        /*
                   slot 0
                 /        \
                /          \
           slot 1 (dead)  slot 2 (dead)
        */
        let blockstore = Blockstore::open(&ledger_path).unwrap();
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 1, 0, blockhash);
        fill_blockstore_slot_with_ticks(&blockstore, ticks_per_slot, 2, 0, blockhash);
        blockstore.set_dead_slot(1).unwrap();
        blockstore.set_dead_slot(2).unwrap();
        let (bank_forks, _leader_schedule) = process_blockstore(
            &genesis_config,
            &blockstore,
            Vec::new(),
            ProcessOptions::default(),
        )
        .unwrap();

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
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
        let mut last_entry_hash = blockhash;

        let blockstore =
            Blockstore::open(&ledger_path).expect("Expected to successfully open database ledger");

        // Let last_slot be the number of slots in the first two epochs
        let epoch_schedule = get_epoch_schedule(&genesis_config, Vec::new());
        let last_slot = epoch_schedule.get_last_slot_in_epoch(1);

        // Create a single chain of slots with all indexes in the range [0, last_slot + 1]
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
        let rooted_slots: Vec<_> = (0..=last_slot).collect();
        blockstore.set_roots(&rooted_slots).unwrap();

        // Set a root on the next slot of the confrimed epoch
        blockstore.set_roots(&[last_slot + 1]).unwrap();

        // Check that we can properly restart the ledger / leader scheduler doesn't fail
        let opts = ProcessOptions {
            poh_verify: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, _leader_schedule) =
            process_blockstore(&genesis_config, &blockstore, Vec::new(), opts).unwrap();

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
            first_err(&[Ok(()), Err(TransactionError::DuplicateSignature)]),
            Err(TransactionError::DuplicateSignature)
        );
        assert_eq!(
            first_err(&[
                Ok(()),
                Err(TransactionError::DuplicateSignature),
                Err(TransactionError::AccountInUse)
            ]),
            Err(TransactionError::DuplicateSignature)
        );
        assert_eq!(
            first_err(&[
                Ok(()),
                Err(TransactionError::AccountInUse),
                Err(TransactionError::DuplicateSignature)
            ]),
            Err(TransactionError::AccountInUse)
        );
        assert_eq!(
            first_err(&[
                Err(TransactionError::AccountInUse),
                Ok(()),
                Err(TransactionError::DuplicateSignature)
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
        let bank = Arc::new(Bank::new(&genesis_config));
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
        process_entries(&bank, &slot_entries, true, None).unwrap();
        assert_eq!(bank.process_transaction(&tx), Ok(()));
    }

    #[test]
    fn test_process_ledger_simple() {
        solana_logger::setup();
        let leader_pubkey = Pubkey::new_rand();
        let mint = 100;
        let hashes_per_tick = 10;
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config_with_leader(mint, &leader_pubkey, 50);
        genesis_config.poh_config.hashes_per_tick = Some(hashes_per_tick);
        let (ledger_path, mut last_entry_hash) = create_new_tmp_ledger!(&genesis_config);
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

        let blockstore =
            Blockstore::open(&ledger_path).expect("Expected to successfully open database ledger");
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
            ..ProcessOptions::default()
        };
        let (bank_forks, _leader_schedule) =
            process_blockstore(&genesis_config, &blockstore, Vec::new(), opts).unwrap();

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
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);

        let blockstore = Blockstore::open(&ledger_path).unwrap();
        let opts = ProcessOptions {
            poh_verify: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, _leader_schedule) =
            process_blockstore(&genesis_config, &blockstore, Vec::new(), opts).unwrap();

        assert_eq!(frozen_bank_slots(&bank_forks), vec![0]);
        let bank = bank_forks[0].clone();
        assert_eq!(bank.tick_height(), 1);
    }

    #[test]
    fn test_process_ledger_options_override_threads() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(123);
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);

        let blockstore = Blockstore::open(&ledger_path).unwrap();
        let opts = ProcessOptions {
            override_num_threads: Some(1),
            ..ProcessOptions::default()
        };
        process_blockstore(&genesis_config, &blockstore, Vec::new(), opts).unwrap();
        PAR_THREAD_POOL.with(|pool| {
            assert_eq!(pool.borrow().current_num_threads(), 1);
        });
    }

    #[test]
    fn test_process_ledger_options_full_leader_cache() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(123);
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);

        let blockstore = Blockstore::open(&ledger_path).unwrap();
        let opts = ProcessOptions {
            full_leader_cache: true,
            ..ProcessOptions::default()
        };
        let (_bank_forks, leader_schedule) =
            process_blockstore(&genesis_config, &blockstore, Vec::new(), opts).unwrap();
        assert_eq!(leader_schedule.max_schedules(), std::usize::MAX);
    }

    #[test]
    fn test_process_ledger_options_entry_callback() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let (ledger_path, last_entry_hash) = create_new_tmp_ledger!(&genesis_config);
        let blockstore =
            Blockstore::open(&ledger_path).expect("Expected to successfully open database ledger");
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
            ..ProcessOptions::default()
        };
        process_blockstore(&genesis_config, &blockstore, Vec::new(), opts).unwrap();
        assert_eq!(*callback_counter.write().unwrap(), 2);
    }

    #[test]
    fn test_process_entries_tick() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(1000);
        let bank = Arc::new(Bank::new(&genesis_config));

        // ensure bank can process a tick
        assert_eq!(bank.tick_height(), 0);
        let tick = next_entry(&genesis_config.hash(), 1, vec![]);
        assert_eq!(process_entries(&bank, &[tick], true, None), Ok(()));
        assert_eq!(bank.tick_height(), 1);
    }

    #[test]
    fn test_process_entries_2_entries_collision() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(1000);
        let bank = Arc::new(Bank::new(&genesis_config));
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
            process_entries(&bank, &[entry_1, entry_2], true, None),
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
        let bank = Arc::new(Bank::new(&genesis_config));
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
            process_entries(
                &bank,
                &[entry_1_to_mint, entry_2_to_3_mint_to_1],
                false,
                None
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
        let bank = Arc::new(Bank::new(&genesis_config));
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

        assert!(process_entries(
            &bank,
            &[entry_1_to_mint.clone(), entry_2_to_3_mint_to_1.clone()],
            false,
            None,
        )
        .is_err());

        // First transaction in first entry succeeded, so keypair1 lost 1 lamport
        assert_eq!(bank.get_balance(&keypair1.pubkey()), 3);
        assert_eq!(bank.get_balance(&keypair2.pubkey()), 4);

        // Check all accounts are unlocked
        let txs1 = &entry_1_to_mint.transactions[..];
        let txs2 = &entry_2_to_3_mint_to_1.transactions[..];
        let batch1 = bank.prepare_batch(txs1, None);
        for result in batch1.lock_results() {
            assert!(result.is_ok());
        }
        // txs1 and txs2 have accounts that conflict, so we must drop txs1 first
        drop(batch1);
        let batch2 = bank.prepare_batch(txs2, None);
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
        let bank = Arc::new(Bank::new(&genesis_config));
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

        assert!(process_entries(
            &bank,
            &[
                entry_1_to_mint,
                entry_2_to_3_and_1_to_mint,
                entry_conflict_itself,
            ],
            false,
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
        let bank = Arc::new(Bank::new(&genesis_config));
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
            process_entries(&bank, &[entry_1, entry_2], true, None),
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
        let bank = Arc::new(Bank::new(&genesis_config));

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
        let present_account = Account::new(1, 10, &Pubkey::default());
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
                    &Pubkey::new_rand(),
                ));

                next_entry_mut(&mut hash, 0, transactions)
            })
            .collect();
        assert_eq!(process_entries(&bank, &entries, true, None), Ok(()));
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

        let bank = Arc::new(Bank::new(&genesis_config));

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
        assert_eq!(process_entries(&bank, &[entry], true, None), Ok(()));
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
        let bank = Arc::new(Bank::new(&genesis_config));
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
            process_entries(&bank, &[entry_1, tick, entry_2.clone()], true, None),
            Ok(())
        );
        assert_eq!(bank.get_balance(&keypair3.pubkey()), 1);
        assert_eq!(bank.get_balance(&keypair4.pubkey()), 1);

        // ensure that an error is returned for an empty account (keypair2)
        let tx =
            system_transaction::transfer(&keypair2, &keypair3.pubkey(), 1, bank.last_blockhash());
        let entry_3 = next_entry(&entry_2.hash, 1, vec![tx]);
        assert_eq!(
            process_entries(&bank, &[entry_3], true, None),
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
        let bank = Arc::new(Bank::new(&genesis_config));
        let pubkey = Pubkey::new_rand();
        bank.transfer(1_000, &mint_keypair, &pubkey).unwrap();
        assert_eq!(bank.transaction_count(), 1);
        assert_eq!(bank.get_balance(&pubkey), 1_000);
        assert_eq!(
            bank.transfer(10_001, &mint_keypair, &pubkey),
            Err(TransactionError::InstructionError(
                0,
                InstructionError::new_result_with_negative_lamports(),
            ))
        );
        assert_eq!(
            bank.transfer(10_001, &mint_keypair, &pubkey),
            Err(TransactionError::DuplicateSignature)
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
        let bank = Arc::new(Bank::new(&genesis_config));
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
            process_entries(&bank, &[entry_1_to_mint], false, None),
            Err(TransactionError::AccountInUse)
        );

        // Should not see duplicate signature error
        assert_eq!(bank.process_transaction(&fail_tx), Ok(()));
    }

    #[test]
    fn test_process_blockstore_from_root() {
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config(123);

        let ticks_per_slot = 1;
        genesis_config.ticks_per_slot = ticks_per_slot;
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
        let blockstore = Blockstore::open(&ledger_path).unwrap();

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
        blockstore.set_roots(&[3, 5]).unwrap();

        // Set up bank1
        let bank0 = Arc::new(Bank::new(&genesis_config));
        let opts = ProcessOptions {
            poh_verify: true,
            ..ProcessOptions::default()
        };
        let recyclers = VerifyRecyclers::default();
        process_bank_0(&bank0, &blockstore, &opts, &recyclers).unwrap();
        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 1));
        confirm_full_slot(
            &blockstore,
            &bank1,
            &opts,
            &recyclers,
            &mut ConfirmationProgress::new(bank0.last_blockhash()),
        )
        .unwrap();
        bank1.squash();

        // Test process_blockstore_from_root() from slot 1 onwards
        let (bank_forks, _leader_schedule) =
            process_blockstore_from_root(&genesis_config, &blockstore, bank1, &opts, &recyclers)
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
        let mut bank = Arc::new(Bank::new(&genesis_config));

        const NUM_TRANSFERS_PER_ENTRY: usize = 8;
        const NUM_TRANSFERS: usize = NUM_TRANSFERS_PER_ENTRY * 32;

        let keypairs: Vec<_> = (0..NUM_TRANSFERS * 2).map(|_| Keypair::new()).collect();

        // give everybody one lamport
        for keypair in &keypairs {
            bank.transfer(1, &mint_keypair, &keypair.pubkey())
                .expect("funding failed");
        }

        let present_account_key = Keypair::new();
        let present_account = Account::new(1, 10, &Pubkey::default());
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
                            &Pubkey::new_rand(),
                        ));
                        transactions
                    })
                })
                .collect();
            info!("paying iteration {}", i);
            process_entries(&bank, &entries, true, None).expect("paying failed");

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
            process_entries(&bank, &entries, true, None).expect("refunding failed");

            // advance to next block
            process_entries(
                &bank,
                &(0..bank.ticks_per_slot())
                    .map(|_| next_entry_mut(&mut hash, 1, vec![]))
                    .collect::<Vec<_>>(),
                true,
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
        let bank0 = Arc::new(Bank::new(&genesis_config));
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

        process_entries_with_callback(&bank0, &entries, true, None, None).unwrap();
        assert_eq!(bank0.get_balance(&keypair.pubkey()), 1)
    }

    fn get_epoch_schedule(
        genesis_config: &GenesisConfig,
        account_paths: Vec<PathBuf>,
    ) -> EpochSchedule {
        let bank = Bank::new_with_paths(&genesis_config, account_paths, &[]);
        *bank.epoch_schedule()
    }

    fn frozen_bank_slots(bank_forks: &BankForks) -> Vec<Slot> {
        let mut slots: Vec<_> = bank_forks.frozen_banks().keys().cloned().collect();
        slots.sort();
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
}

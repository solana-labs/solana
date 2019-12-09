use crate::{
    bank_forks::BankForks,
    block_error::BlockError,
    blocktree::Blocktree,
    blocktree_meta::SlotMeta,
    entry::{create_ticks, Entry, EntrySlice},
    leader_schedule_cache::LeaderScheduleCache,
};
use crossbeam_channel::Sender;
use itertools::Itertools;
use log::*;
use rand::{seq::SliceRandom, thread_rng};
use rayon::{prelude::*, ThreadPool};
use solana_metrics::{datapoint, datapoint_error, inc_new_counter_debug};
use solana_rayon_threadlimit::get_thread_count;
use solana_runtime::{
    bank::{Bank, TransactionProcessResult, TransactionResults},
    transaction_batch::TransactionBatch,
};
use solana_sdk::{
    clock::{Slot, MAX_RECENT_BLOCKHASHES},
    genesis_config::GenesisConfig,
    hash::Hash,
    signature::{Keypair, KeypairUtil},
    timing::duration_as_ms,
    transaction::{Result, Transaction},
};
use std::{
    cell::RefCell,
    path::PathBuf,
    result,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;

thread_local!(static PAR_THREAD_POOL: RefCell<ThreadPool> = RefCell::new(rayon::ThreadPoolBuilder::new()
                    .num_threads(get_thread_count())
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
    let TransactionResults {
        fee_collection_results,
        processing_results,
    } = batch
        .bank()
        .load_execute_and_commit_transactions(batch, MAX_RECENT_BLOCKHASHES);

    if let Some(sender) = transaction_status_sender {
        send_transaction_status_batch(
            bank.clone(),
            batch.transactions(),
            processing_results,
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
                datapoint!(
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

#[derive(Debug, PartialEq)]
pub struct BankForksInfo {
    pub bank_slot: u64,
}

#[derive(Error, Debug, PartialEq)]
pub enum BlocktreeProcessorError {
    #[error("failed to load entries")]
    FailedToLoadEntries,

    #[error("failed to load meta")]
    FailedToLoadMeta,

    #[error("invalid block")]
    InvalidBlock(#[from] BlockError),

    #[error("invalid transaction")]
    InvalidTransaction,

    #[error("no valid forks found")]
    NoValidForksFound,
}

/// Callback for accessing bank state while processing the blocktree
pub type ProcessCallback = Arc<dyn Fn(&Bank) -> () + Sync + Send>;

#[derive(Default)]
pub struct ProcessOptions {
    pub poh_verify: bool,
    pub full_leader_cache: bool,
    pub dev_halt_at_slot: Option<Slot>,
    pub entry_callback: Option<ProcessCallback>,
    pub override_num_threads: Option<usize>,
}

pub fn process_blocktree(
    genesis_config: &GenesisConfig,
    blocktree: &Blocktree,
    account_paths: Vec<PathBuf>,
    opts: ProcessOptions,
) -> result::Result<(BankForks, Vec<BankForksInfo>, LeaderScheduleCache), BlocktreeProcessorError> {
    if let Some(num_threads) = opts.override_num_threads {
        PAR_THREAD_POOL.with(|pool| {
            *pool.borrow_mut() = rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .unwrap()
        });
    }

    // Setup bank for slot 0
    let bank0 = Arc::new(Bank::new_with_paths(&genesis_config, account_paths));
    info!("processing ledger for bank 0...");
    process_bank_0(&bank0, blocktree, &opts)?;
    process_blocktree_from_root(genesis_config, blocktree, bank0, &opts)
}

// Process blocktree from a known root bank
pub fn process_blocktree_from_root(
    genesis_config: &GenesisConfig,
    blocktree: &Blocktree,
    bank: Arc<Bank>,
    opts: &ProcessOptions,
) -> result::Result<(BankForks, Vec<BankForksInfo>, LeaderScheduleCache), BlocktreeProcessorError> {
    info!("processing ledger from root: {}...", bank.slot());
    // Starting slot must be a root, and thus has no parents
    assert!(bank.parent().is_none());
    let start_slot = bank.slot();
    let now = Instant::now();
    let mut rooted_path = vec![start_slot];

    bank.set_entered_epoch_callback(solana_genesis_programs::get_entered_epoch_callback(
        genesis_config.operating_mode,
    ));

    blocktree
        .set_roots(&[start_slot])
        .expect("Couldn't set root on startup");

    let meta = blocktree.meta(start_slot).unwrap();

    // Iterate and replay slots from blocktree starting from `start_slot`
    let (bank_forks, bank_forks_info, leader_schedule_cache) = {
        if let Some(meta) = meta {
            let epoch_schedule = bank.epoch_schedule();
            let mut leader_schedule_cache = LeaderScheduleCache::new(*epoch_schedule, &bank);
            if opts.full_leader_cache {
                leader_schedule_cache.set_max_schedules(std::usize::MAX);
            }
            let fork_info = process_pending_slots(
                &bank,
                &meta,
                blocktree,
                &mut leader_schedule_cache,
                &mut rooted_path,
                opts,
            )?;
            let (banks, bank_forks_info): (Vec<_>, Vec<_>) = fork_info.into_iter().unzip();
            if banks.is_empty() {
                return Err(BlocktreeProcessorError::NoValidForksFound);
            }
            let bank_forks = BankForks::new_from_banks(&banks, rooted_path);
            (bank_forks, bank_forks_info, leader_schedule_cache)
        } else {
            // If there's no meta for the input `start_slot`, then we started from a snapshot
            // and there's no point in processing the rest of blocktree and implies blocktree
            // should be empty past this point.
            let bfi = BankForksInfo {
                bank_slot: start_slot,
            };
            let leader_schedule_cache = LeaderScheduleCache::new_from_bank(&bank);
            let bank_forks = BankForks::new_from_banks(&[bank], rooted_path);
            (bank_forks, vec![bfi], leader_schedule_cache)
        }
    };

    info!(
        "ledger processed in {}ms. {} fork{} at {}",
        duration_as_ms(&now.elapsed()),
        bank_forks_info.len(),
        if bank_forks_info.len() > 1 { "s" } else { "" },
        bank_forks_info
            .iter()
            .map(|bfi| bfi.bank_slot.to_string())
            .join(", ")
    );

    Ok((bank_forks, bank_forks_info, leader_schedule_cache))
}

fn verify_and_process_slot_entries(
    bank: &Arc<Bank>,
    entries: &[Entry],
    last_entry_hash: Hash,
    opts: &ProcessOptions,
) -> result::Result<Hash, BlocktreeProcessorError> {
    assert!(!entries.is_empty());

    if opts.poh_verify {
        let next_bank_tick_height = bank.tick_height() + entries.tick_count();
        let max_bank_tick_height = bank.max_tick_height();
        if next_bank_tick_height != max_bank_tick_height {
            warn!(
                "Invalid number of entry ticks found in slot: {}",
                bank.slot()
            );
            return Err(BlockError::InvalidTickCount.into());
        } else if !entries.last().unwrap().is_tick() {
            warn!("Slot: {} did not end with a tick entry", bank.slot());
            return Err(BlockError::TrailingEntry.into());
        }

        if let Some(hashes_per_tick) = bank.hashes_per_tick() {
            if !entries.verify_tick_hash_count(&mut 0, *hashes_per_tick) {
                warn!(
                    "Tick with invalid number of hashes found in slot: {}",
                    bank.slot()
                );
                return Err(BlockError::InvalidTickHashCount.into());
            }
        }

        if !entries.verify(&last_entry_hash) {
            warn!("Ledger proof of history failed at slot: {}", bank.slot());
            return Err(BlockError::InvalidEntryHash.into());
        }
    }

    process_entries_with_callback(bank, &entries, true, opts.entry_callback.as_ref(), None)
        .map_err(|err| {
            warn!(
                "Failed to process entries for slot {}: {:?}",
                bank.slot(),
                err
            );
            BlocktreeProcessorError::InvalidTransaction
        })?;

    Ok(entries.last().unwrap().hash)
}

// Special handling required for processing the entries in slot 0
fn process_bank_0(
    bank0: &Arc<Bank>,
    blocktree: &Blocktree,
    opts: &ProcessOptions,
) -> result::Result<(), BlocktreeProcessorError> {
    assert_eq!(bank0.slot(), 0);

    // Fetch all entries for this slot
    let entries = blocktree.get_slot_entries(0, 0, None).map_err(|err| {
        warn!("Failed to load entries for slot 0, err: {:?}", err);
        BlocktreeProcessorError::FailedToLoadEntries
    })?;

    verify_and_process_slot_entries(bank0, &entries, bank0.last_blockhash(), opts)?;

    bank0.freeze();

    Ok(())
}

// Given a slot, add its children to the pending slots queue if those children slots are
// complete
fn process_next_slots(
    bank: &Arc<Bank>,
    meta: &SlotMeta,
    blocktree: &Blocktree,
    leader_schedule_cache: &LeaderScheduleCache,
    pending_slots: &mut Vec<(u64, SlotMeta, Arc<Bank>, Hash)>,
    fork_info: &mut Vec<(Arc<Bank>, BankForksInfo)>,
) -> result::Result<(), BlocktreeProcessorError> {
    if meta.next_slots.is_empty() {
        // Reached the end of this fork.  Record the final entry height and last entry.hash
        let bfi = BankForksInfo {
            bank_slot: bank.slot(),
        };
        fork_info.push((bank.clone(), bfi));
        return Ok(());
    }

    // This is a fork point if there are multiple children, create a new child bank for each fork
    for next_slot in &meta.next_slots {
        let next_meta = blocktree
            .meta(*next_slot)
            .map_err(|err| {
                warn!("Failed to load meta for slot {}: {:?}", next_slot, err);
                BlocktreeProcessorError::FailedToLoadMeta
            })?
            .unwrap();

        // Only process full slots in blocktree_processor, replay_stage
        // handles any partials
        if next_meta.is_full() {
            let next_bank = Arc::new(Bank::new_from_parent(
                &bank,
                &leader_schedule_cache
                    .slot_leader_at(*next_slot, Some(&bank))
                    .unwrap(),
                *next_slot,
            ));
            trace!("Add child bank {} of slot={}", next_slot, bank.slot());
            pending_slots.push((*next_slot, next_meta, next_bank, bank.last_blockhash()));
        } else {
            let bfi = BankForksInfo {
                bank_slot: bank.slot(),
            };
            fork_info.push((bank.clone(), bfi));
        }
    }

    // Reverse sort by slot, so the next slot to be processed can be popped
    pending_slots.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(())
}

// Iterate through blocktree processing slots starting from the root slot pointed to by the
// given `meta`
fn process_pending_slots(
    root_bank: &Arc<Bank>,
    root_meta: &SlotMeta,
    blocktree: &Blocktree,
    leader_schedule_cache: &mut LeaderScheduleCache,
    rooted_path: &mut Vec<u64>,
    opts: &ProcessOptions,
) -> result::Result<Vec<(Arc<Bank>, BankForksInfo)>, BlocktreeProcessorError> {
    let mut fork_info = vec![];
    let mut last_status_report = Instant::now();
    let mut pending_slots = vec![];
    process_next_slots(
        root_bank,
        root_meta,
        blocktree,
        leader_schedule_cache,
        &mut pending_slots,
        &mut fork_info,
    )?;

    let dev_halt_at_slot = opts.dev_halt_at_slot.unwrap_or(std::u64::MAX);
    while !pending_slots.is_empty() {
        let (slot, meta, bank, last_entry_hash) = pending_slots.pop().unwrap();

        if last_status_report.elapsed() > Duration::from_secs(2) {
            info!("processing ledger...block {}", slot);
            last_status_report = Instant::now();
        }

        if blocktree.is_dead(slot) {
            warn!("slot {} is dead", slot);
            continue;
        }

        // Fetch all entries for this slot
        let entries = blocktree.get_slot_entries(slot, 0, None).map_err(|err| {
            warn!("Failed to load entries for slot {}: {:?}", slot, err);
            BlocktreeProcessorError::FailedToLoadEntries
        })?;

        if let Err(err) = verify_and_process_slot_entries(&bank, &entries, last_entry_hash, opts) {
            warn!("slot {} failed to verify: {}", slot, err);
            continue;
        }

        bank.freeze(); // all banks handled by this routine are created from complete slots

        if blocktree.is_root(slot) {
            let parents = bank.parents().into_iter().map(|b| b.slot()).rev().skip(1);
            let parents: Vec<_> = parents.collect();
            rooted_path.extend(parents);
            rooted_path.push(slot);
            leader_schedule_cache.set_root(&bank);
            bank.squash();
            pending_slots.clear();
            fork_info.clear();
        }

        if slot >= dev_halt_at_slot {
            let bfi = BankForksInfo { bank_slot: slot };
            fork_info.push((bank, bfi));
            break;
        }

        process_next_slots(
            &bank,
            &meta,
            blocktree,
            leader_schedule_cache,
            &mut pending_slots,
            &mut fork_info,
        )?;
    }

    Ok(fork_info)
}

pub struct TransactionStatusBatch {
    pub bank: Arc<Bank>,
    pub transactions: Vec<Transaction>,
    pub statuses: Vec<TransactionProcessResult>,
}
pub type TransactionStatusSender = Sender<TransactionStatusBatch>;

pub fn send_transaction_status_batch(
    bank: Arc<Bank>,
    transactions: &[Transaction],
    statuses: Vec<TransactionProcessResult>,
    transaction_status_sender: TransactionStatusSender,
) {
    let slot = bank.slot();
    if let Err(e) = transaction_status_sender.send(TransactionStatusBatch {
        bank,
        transactions: transactions.to_vec(),
        statuses,
    }) {
        trace!(
            "Slot {} transaction_status send batch failed: {:?}",
            slot,
            e
        );
    }
}

// used for tests only
pub fn fill_blocktree_slot_with_ticks(
    blocktree: &Blocktree,
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

    blocktree
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
        signature::{Keypair, KeypairUtil},
        system_transaction,
        transaction::{Transaction, TransactionError},
    };
    use std::sync::RwLock;

    #[test]
    fn test_process_blocktree_with_missing_hashes() {
        solana_logger::setup();

        let hashes_per_tick = 2;
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config(10_000);
        genesis_config.poh_config.hashes_per_tick = Some(hashes_per_tick);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
        let blocktree =
            Blocktree::open(&ledger_path).expect("Expected to successfully open database ledger");

        let parent_slot = 0;
        let slot = 1;
        let entries = create_ticks(ticks_per_slot, hashes_per_tick - 1, blockhash);
        assert_matches!(
            blocktree.write_entries(
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

        assert_eq!(
            process_blocktree(
                &genesis_config,
                &blocktree,
                Vec::new(),
                ProcessOptions {
                    poh_verify: true,
                    ..ProcessOptions::default()
                }
            )
            .err(),
            Some(BlocktreeProcessorError::NoValidForksFound)
        );
    }

    #[test]
    fn test_process_blocktree_with_invalid_slot_tick_count() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
        let blocktree = Blocktree::open(&ledger_path).unwrap();

        // Write slot 1 with one tick missing
        let parent_slot = 0;
        let slot = 1;
        let entries = create_ticks(ticks_per_slot - 1, 0, blockhash);
        assert_matches!(
            blocktree.write_entries(
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

        // No valid forks in blocktree, expect a failure
        assert_eq!(
            process_blocktree(
                &genesis_config,
                &blocktree,
                Vec::new(),
                ProcessOptions {
                    poh_verify: true,
                    ..ProcessOptions::default()
                }
            )
            .err(),
            Some(BlocktreeProcessorError::NoValidForksFound)
        );

        // Write slot 2 fully
        let _last_slot2_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 2, 0, blockhash);

        let (_bank_forks, bank_forks_info, _) = process_blocktree(
            &genesis_config,
            &blocktree,
            Vec::new(),
            ProcessOptions {
                poh_verify: true,
                ..ProcessOptions::default()
            },
        )
        .unwrap();

        // One valid fork, one bad fork.  process_blocktree() should only return the valid fork
        assert_eq!(bank_forks_info, vec![BankForksInfo { bank_slot: 2 }]);
    }

    #[test]
    fn test_process_blocktree_with_slot_with_trailing_entry() {
        solana_logger::setup();

        let GenesisConfigInfo {
            mint_keypair,
            genesis_config,
            ..
        } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
        let blocktree = Blocktree::open(&ledger_path).unwrap();

        let mut entries = create_ticks(ticks_per_slot, 0, blockhash);
        let trailing_entry = {
            let keypair = Keypair::new();
            let tx = system_transaction::transfer(&mint_keypair, &keypair.pubkey(), 1, blockhash);
            next_entry(&blockhash, 1, vec![tx])
        };
        entries.push(trailing_entry);

        // Tricks blocktree into writing the trailing entry by lying that there is one more tick
        // per slot.
        let parent_slot = 0;
        let slot = 1;
        assert_matches!(
            blocktree.write_entries(
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
        assert_eq!(
            process_blocktree(&genesis_config, &blocktree, Vec::new(), opts).err(),
            Some(BlocktreeProcessorError::NoValidForksFound)
        );
    }

    #[test]
    fn test_process_blocktree_with_incomplete_slot() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        /*
          Build a blocktree in the ledger with the following fork structure:

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

        let blocktree =
            Blocktree::open(&ledger_path).expect("Expected to successfully open database ledger");

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
                blocktree.write_entries(
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
        fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 2, 1, blockhash);

        let opts = ProcessOptions {
            poh_verify: true,
            ..ProcessOptions::default()
        };
        let (mut _bank_forks, bank_forks_info, _) =
            process_blocktree(&genesis_config, &blocktree, Vec::new(), opts).unwrap();

        assert_eq!(bank_forks_info.len(), 1);
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_slot: 0, // slot 1 isn't "full", we stop at slot zero
            }
        );
    }

    #[test]
    fn test_process_blocktree_with_two_forks_and_squash() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
        debug!("ledger_path: {:?}", ledger_path);
        let mut last_entry_hash = blockhash;

        /*
            Build a blocktree in the ledger with the following fork structure:

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
        let blocktree =
            Blocktree::open(&ledger_path).expect("Expected to successfully open database ledger");

        // Fork 1, ending at slot 3
        let last_slot1_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 1, 0, last_entry_hash);
        last_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 2, 1, last_slot1_entry_hash);
        let last_fork1_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 3, 2, last_entry_hash);

        // Fork 2, ending at slot 4
        let last_fork2_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 4, 1, last_slot1_entry_hash);

        info!("last_fork1_entry.hash: {:?}", last_fork1_entry_hash);
        info!("last_fork2_entry.hash: {:?}", last_fork2_entry_hash);

        blocktree.set_roots(&[0, 1, 4]).unwrap();

        let opts = ProcessOptions {
            poh_verify: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, bank_forks_info, _) =
            process_blocktree(&genesis_config, &blocktree, Vec::new(), opts).unwrap();

        assert_eq!(bank_forks_info.len(), 1); // One fork, other one is ignored b/c not a descendant of the root

        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_slot: 4, // Fork 2's head is slot 4
            }
        );
        assert!(&bank_forks[4]
            .parents()
            .iter()
            .map(|bank| bank.slot())
            .collect::<Vec<_>>()
            .is_empty());

        // Ensure bank_forks holds the right banks
        verify_fork_infos(&bank_forks, &bank_forks_info);

        assert_eq!(bank_forks.root(), 4);
    }

    #[test]
    fn test_process_blocktree_with_two_forks() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
        debug!("ledger_path: {:?}", ledger_path);
        let mut last_entry_hash = blockhash;

        /*
            Build a blocktree in the ledger with the following fork structure:

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
        let blocktree =
            Blocktree::open(&ledger_path).expect("Expected to successfully open database ledger");

        // Fork 1, ending at slot 3
        let last_slot1_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 1, 0, last_entry_hash);
        last_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 2, 1, last_slot1_entry_hash);
        let last_fork1_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 3, 2, last_entry_hash);

        // Fork 2, ending at slot 4
        let last_fork2_entry_hash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 4, 1, last_slot1_entry_hash);

        info!("last_fork1_entry.hash: {:?}", last_fork1_entry_hash);
        info!("last_fork2_entry.hash: {:?}", last_fork2_entry_hash);

        blocktree.set_roots(&[0, 1]).unwrap();

        let opts = ProcessOptions {
            poh_verify: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, bank_forks_info, _) =
            process_blocktree(&genesis_config, &blocktree, Vec::new(), opts).unwrap();

        assert_eq!(bank_forks_info.len(), 2); // There are two forks
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_slot: 3, // Fork 1's head is slot 3
            }
        );
        assert_eq!(
            &bank_forks[3]
                .parents()
                .iter()
                .map(|bank| bank.slot())
                .collect::<Vec<_>>(),
            &[2, 1]
        );
        assert_eq!(
            bank_forks_info[1],
            BankForksInfo {
                bank_slot: 4, // Fork 2's head is slot 4
            }
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
        verify_fork_infos(&bank_forks, &bank_forks_info);
    }

    #[test]
    fn test_process_blocktree_with_dead_slot() {
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
        let blocktree = Blocktree::open(&ledger_path).unwrap();
        let slot1_blockhash =
            fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 1, 0, blockhash);
        fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 2, 1, slot1_blockhash);
        blocktree.set_dead_slot(2).unwrap();
        fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, 3, 1, slot1_blockhash);

        let (bank_forks, bank_forks_info, _) = process_blocktree(
            &genesis_config,
            &blocktree,
            Vec::new(),
            ProcessOptions::default(),
        )
        .unwrap();

        assert_eq!(bank_forks_info.len(), 1);
        assert_eq!(bank_forks_info[0], BankForksInfo { bank_slot: 3 });
        assert_eq!(
            &bank_forks[3]
                .parents()
                .iter()
                .map(|bank| bank.slot())
                .collect::<Vec<_>>(),
            &[1, 0]
        );
        verify_fork_infos(&bank_forks, &bank_forks_info);
    }

    #[test]
    fn test_process_blocktree_epoch_boundary_root() {
        solana_logger::setup();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let ticks_per_slot = genesis_config.ticks_per_slot;

        // Create a new ledger with slot 0 full of ticks
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
        let mut last_entry_hash = blockhash;

        let blocktree =
            Blocktree::open(&ledger_path).expect("Expected to successfully open database ledger");

        // Let last_slot be the number of slots in the first two epochs
        let epoch_schedule = get_epoch_schedule(&genesis_config, Vec::new());
        let last_slot = epoch_schedule.get_last_slot_in_epoch(1);

        // Create a single chain of slots with all indexes in the range [0, last_slot + 1]
        for i in 1..=last_slot + 1 {
            last_entry_hash = fill_blocktree_slot_with_ticks(
                &blocktree,
                ticks_per_slot,
                i,
                i - 1,
                last_entry_hash,
            );
        }

        // Set a root on the last slot of the last confirmed epoch
        let rooted_slots: Vec<_> = (0..=last_slot).collect();
        blocktree.set_roots(&rooted_slots).unwrap();

        // Set a root on the next slot of the confrimed epoch
        blocktree.set_roots(&[last_slot + 1]).unwrap();

        // Check that we can properly restart the ledger / leader scheduler doesn't fail
        let opts = ProcessOptions {
            poh_verify: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, bank_forks_info, _) =
            process_blocktree(&genesis_config, &blocktree, Vec::new(), opts).unwrap();

        assert_eq!(bank_forks_info.len(), 1); // There is one fork
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_slot: last_slot + 1, // Head is last_slot + 1
            }
        );

        // The latest root should have purged all its parents
        assert!(&bank_forks[last_slot + 1]
            .parents()
            .iter()
            .map(|bank| bank.slot())
            .collect::<Vec<_>>()
            .is_empty());
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

        let blocktree =
            Blocktree::open(&ledger_path).expect("Expected to successfully open database ledger");
        blocktree
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
        let (bank_forks, bank_forks_info, _) =
            process_blocktree(&genesis_config, &blocktree, Vec::new(), opts).unwrap();

        assert_eq!(bank_forks_info.len(), 1);
        assert_eq!(bank_forks.root(), 0);
        assert_eq!(bank_forks_info[0], BankForksInfo { bank_slot: 1 });

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

        let blocktree = Blocktree::open(&ledger_path).unwrap();
        let opts = ProcessOptions {
            poh_verify: true,
            ..ProcessOptions::default()
        };
        let (bank_forks, bank_forks_info, _) =
            process_blocktree(&genesis_config, &blocktree, Vec::new(), opts).unwrap();

        assert_eq!(bank_forks_info.len(), 1);
        assert_eq!(bank_forks_info[0], BankForksInfo { bank_slot: 0 });
        let bank = bank_forks[0].clone();
        assert_eq!(bank.tick_height(), 1);
    }

    #[test]
    fn test_process_ledger_options_override_threads() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(123);
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);

        let blocktree = Blocktree::open(&ledger_path).unwrap();
        let opts = ProcessOptions {
            override_num_threads: Some(1),
            ..ProcessOptions::default()
        };
        process_blocktree(&genesis_config, &blocktree, Vec::new(), opts).unwrap();
        PAR_THREAD_POOL.with(|pool| {
            assert_eq!(pool.borrow().current_num_threads(), 1);
        });
    }

    #[test]
    fn test_process_ledger_options_full_leader_cache() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(123);
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_config);

        let blocktree = Blocktree::open(&ledger_path).unwrap();
        let opts = ProcessOptions {
            full_leader_cache: true,
            ..ProcessOptions::default()
        };
        let (_bank_forks, _bank_forks_info, cached_leader_schedule) =
            process_blocktree(&genesis_config, &blocktree, Vec::new(), opts).unwrap();
        assert_eq!(cached_leader_schedule.max_schedules(), std::usize::MAX);
    }

    #[test]
    fn test_process_ledger_options_entry_callback() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let (ledger_path, last_entry_hash) = create_new_tmp_ledger!(&genesis_config);
        let blocktree =
            Blocktree::open(&ledger_path).expect("Expected to successfully open database ledger");
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
        blocktree
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
        process_blocktree(&genesis_config, &blocktree, Vec::new(), opts).unwrap();
        assert_eq!(*callback_counter.write().unwrap(), 2);
    }

    #[test]
    fn test_process_entries_tick() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(1000);
        let bank = Arc::new(Bank::new(&genesis_config));

        // ensure bank can process a tick
        assert_eq!(bank.tick_height(), 0);
        let tick = next_entry(&genesis_config.hash(), 1, vec![]);
        assert_eq!(process_entries(&bank, &[tick.clone()], true, None), Ok(()));
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
                entry_1_to_mint.clone(),
                entry_2_to_3_and_1_to_mint.clone(),
                entry_conflict_itself.clone()
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
        assert_eq!(process_entries(&bank, &vec![entry], true, None), Ok(()));
        bank.squash();

        // Even number keypair should have balance of 2 * initial_lamports and
        // odd number keypair should have balance of 0, which proves
        // that even in case of random order of execution, overall state remains
        // consistent.
        for i in 0..num_accounts {
            if i % 2 == 0 {
                assert_eq!(
                    bank.get_balance(&keypairs[i].pubkey()),
                    2 * initial_lamports
                );
            } else {
                assert_eq!(bank.get_balance(&keypairs[i].pubkey()), 0);
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
            process_entries(
                &bank,
                &[entry_1.clone(), tick.clone(), entry_2.clone()],
                true,
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
    fn test_process_blocktree_from_root() {
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config(123);

        let ticks_per_slot = 1;
        genesis_config.ticks_per_slot = ticks_per_slot;
        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);
        let blocktree = Blocktree::open(&ledger_path).unwrap();

        /*
          Build a blocktree in the ledger with the following fork structure:

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
                fill_blocktree_slot_with_ticks(&blocktree, ticks_per_slot, i + 1, i, last_hash);
        }
        blocktree.set_roots(&[3, 5]).unwrap();

        // Set up bank1
        let bank0 = Arc::new(Bank::new(&genesis_config));
        let opts = ProcessOptions {
            poh_verify: true,
            ..ProcessOptions::default()
        };
        process_bank_0(&bank0, &blocktree, &opts).unwrap();
        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &Pubkey::default(), 1));
        let slot1_entries = blocktree.get_slot_entries(1, 0, None).unwrap();
        verify_and_process_slot_entries(&bank1, &slot1_entries, bank0.last_blockhash(), &opts)
            .unwrap();
        bank1.squash();

        // Test process_blocktree_from_root() from slot 1 onwards
        let (bank_forks, bank_forks_info, _) =
            process_blocktree_from_root(&genesis_config, &blocktree, bank1, &opts).unwrap();

        assert_eq!(bank_forks_info.len(), 1); // One fork
        assert_eq!(
            bank_forks_info[0],
            BankForksInfo {
                bank_slot: 6, // The head of the fork is slot 6
            }
        );
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
        verify_fork_infos(&bank_forks, &bank_forks_info);
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
                root.map(|old_root| old_root.squash());
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
        let bank = Bank::new_with_paths(&genesis_config, account_paths);
        bank.epoch_schedule().clone()
    }

    // Check that `bank_forks` contains all the ancestors and banks for each fork identified in
    // `bank_forks_info`
    fn verify_fork_infos(bank_forks: &BankForks, bank_forks_info: &[BankForksInfo]) {
        for fork in bank_forks_info {
            let head_slot = fork.bank_slot;
            let head_bank = &bank_forks[head_slot];
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

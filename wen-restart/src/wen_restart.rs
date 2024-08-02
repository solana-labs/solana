//! The `wen-restart` module handles automatic repair during a cluster restart

use {
    crate::{
        heaviest_fork_aggregate::HeaviestForkAggregate,
        last_voted_fork_slots_aggregate::{
            LastVotedForkSlotsAggregate, LastVotedForkSlotsFinalResult,
        },
        solana::wen_restart_proto::{
            self, GenerateSnapshotRecord, HeaviestForkAggregateFinal, HeaviestForkAggregateRecord,
            HeaviestForkRecord, LastVotedForkSlotsAggregateFinal,
            LastVotedForkSlotsAggregateRecord, LastVotedForkSlotsRecord, State as RestartState,
            WenRestartProgress,
        },
    },
    anyhow::Result,
    log::*,
    prost::Message,
    solana_entry::entry::VerifyRecyclers,
    solana_gossip::{
        cluster_info::{ClusterInfo, GOSSIP_SLEEP_MILLIS},
        restart_crds_values::RestartLastVotedForkSlots,
    },
    solana_ledger::{
        ancestor_iterator::AncestorIterator,
        blockstore::Blockstore,
        blockstore_processor::{process_single_slot, ConfirmationProgress, ProcessOptions},
        leader_schedule_cache::LeaderScheduleCache,
    },
    solana_program::{clock::Slot, hash::Hash},
    solana_runtime::{
        accounts_background_service::AbsRequestSender,
        bank::Bank,
        bank_forks::BankForks,
        snapshot_archive_info::SnapshotArchiveInfoGetter,
        snapshot_bank_utils::bank_to_incremental_snapshot_archive,
        snapshot_config::SnapshotConfig,
        snapshot_utils::{
            get_highest_full_snapshot_archive_slot, get_highest_incremental_snapshot_archive_slot,
            purge_all_bank_snapshots,
        },
    },
    solana_sdk::{shred_version::compute_shred_version, timing::timestamp},
    solana_timings::ExecuteTimings,
    solana_vote_program::vote_state::VoteTransaction,
    std::{
        collections::{HashMap, HashSet},
        fs::{read, File},
        io::{Cursor, Write},
        path::{Path, PathBuf},
        str::FromStr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::sleep,
        time::{Duration, Instant},
    },
};

// If >42% of the validators have this block, repair this block locally.
const REPAIR_THRESHOLD: f64 = 0.42;
// When counting Heaviest Fork, only count those with no less than
// 67% - 5% - (100% - active_stake) = active_stake - 38% stake.
// 67% is the supermajority threshold (2/3), 5% is the assumption we
// made regarding how much non-conforming/offline validators the
// algorithm can tolerate.
const HEAVIEST_FORK_THRESHOLD_DELTA: f64 = 0.38;
// We allow at most 5% of the stake to disagree with us.
const HEAVIEST_FORK_DISAGREE_THRESHOLD_PERCENT: f64 = 5.0;
// We update HeaviestFork every 5 minutes at least.
const HEAVIEST_REFRESH_INTERVAL_IN_SECONDS: u64 = 300;

#[derive(Debug, PartialEq)]
pub enum WenRestartError {
    BlockNotFound(Slot),
    BlockNotFull(Slot),
    BlockNotFrozenAfterReplay(Slot, Option<String>),
    BlockNotLinkedToExpectedParent(Slot, Option<Slot>, Slot),
    ChildStakeLargerThanParent(Slot, u64, Slot, u64),
    Exiting,
    FutureSnapshotExists(Slot, Slot, String),
    GenerateSnapshotWhenOneExists(Slot, String),
    MalformedLastVotedForkSlotsProtobuf(Option<LastVotedForkSlotsRecord>),
    MalformedProgress(RestartState, String),
    MissingLastVotedForkSlots,
    MissingFullSnapshot(String),
    NotEnoughStakeAgreeingWithUs(Slot, Hash, HashMap<(Slot, Hash), u64>),
    UnexpectedState(wen_restart_proto::State),
}

impl std::fmt::Display for WenRestartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WenRestartError::BlockNotFound(slot) => {
                write!(f, "Block not found: {}", slot)
            }
            WenRestartError::BlockNotFull(slot) => {
                write!(f, "Block not full: {}", slot)
            }
            WenRestartError::BlockNotFrozenAfterReplay(slot, err) => {
                write!(f, "Block not frozen after replay: {} {:?}", slot, err)
            }
            WenRestartError::BlockNotLinkedToExpectedParent(slot, parent, expected_parent) => {
                write!(
                    f,
                    "Block {} is not linked to expected parent {} but to {:?}",
                    slot, expected_parent, parent
                )
            }
            WenRestartError::ChildStakeLargerThanParent(
                slot,
                child_stake,
                parent,
                parent_stake,
            ) => {
                write!(
                    f,
                    "Block {} has more stake {} than its parent {} with stake {}",
                    slot, child_stake, parent, parent_stake
                )
            }
            WenRestartError::Exiting => write!(f, "Exiting"),
            WenRestartError::FutureSnapshotExists(slot, highest_slot, directory) => {
                write!(
                    f,
                    "Future snapshot exists for slot: {slot} highest slot: {highest_slot} in directory: {directory}",
                )
            }
            WenRestartError::GenerateSnapshotWhenOneExists(slot, directory) => {
                write!(
                    f,
                    "Generate snapshot when one exists for slot: {slot} in directory: {directory}",
                )
            }
            WenRestartError::MalformedLastVotedForkSlotsProtobuf(record) => {
                write!(f, "Malformed last voted fork slots protobuf: {:?}", record)
            }
            WenRestartError::MalformedProgress(state, missing) => {
                write!(f, "Malformed progress: {:?} missing {}", state, missing)
            }
            WenRestartError::MissingLastVotedForkSlots => {
                write!(f, "Missing last voted fork slots")
            }
            WenRestartError::MissingFullSnapshot(directory) => {
                write!(f, "Missing full snapshot, please check whether correct directory is supplied {directory}")
            }
            WenRestartError::NotEnoughStakeAgreeingWithUs(slot, hash, block_stake_map) => {
                write!(
                    f,
                    "Not enough stake agreeing with our slot: {} hash: {}\n {:?}",
                    slot, hash, block_stake_map,
                )
            }
            WenRestartError::UnexpectedState(state) => {
                write!(f, "Unexpected state: {:?}", state)
            }
        }
    }
}

impl std::error::Error for WenRestartError {}

// We need a WenRestartProgressInternalState so we can convert the protobuf written in file
// into internal data structure in the initialize function. It should be easily
// convertable to and from WenRestartProgress protobuf.
#[derive(Debug, PartialEq)]
pub(crate) enum WenRestartProgressInternalState {
    Init {
        last_voted_fork_slots: Vec<Slot>,
        last_vote_bankhash: Hash,
    },
    LastVotedForkSlots {
        last_voted_fork_slots: Vec<Slot>,
        aggregate_final_result: Option<LastVotedForkSlotsFinalResult>,
    },
    FindHeaviestFork {
        aggregate_final_result: LastVotedForkSlotsFinalResult,
        my_heaviest_fork: Option<HeaviestForkRecord>,
    },
    HeaviestFork {
        new_root_slot: Slot,
    },
    GenerateSnapshot {
        new_root_slot: Slot,
        my_snapshot: Option<GenerateSnapshotRecord>,
    },
    Done,
}

pub(crate) fn send_restart_last_voted_fork_slots(
    cluster_info: Arc<ClusterInfo>,
    last_voted_fork_slots: &[Slot],
    last_vote_bankhash: Hash,
) -> Result<LastVotedForkSlotsRecord> {
    cluster_info.push_restart_last_voted_fork_slots(last_voted_fork_slots, last_vote_bankhash)?;
    Ok(LastVotedForkSlotsRecord {
        last_voted_fork_slots: last_voted_fork_slots.to_vec(),
        last_vote_bankhash: last_vote_bankhash.to_string(),
        shred_version: cluster_info.my_shred_version() as u32,
        wallclock: timestamp(),
    })
}

pub(crate) fn aggregate_restart_last_voted_fork_slots(
    wen_restart_path: &PathBuf,
    wait_for_supermajority_threshold_percent: u64,
    cluster_info: Arc<ClusterInfo>,
    last_voted_fork_slots: &Vec<Slot>,
    bank_forks: Arc<RwLock<BankForks>>,
    blockstore: Arc<Blockstore>,
    wen_restart_repair_slots: Arc<RwLock<Vec<Slot>>>,
    exit: Arc<AtomicBool>,
    progress: &mut WenRestartProgress,
) -> Result<LastVotedForkSlotsFinalResult> {
    let root_bank = bank_forks.read().unwrap().root_bank();
    let root_slot = root_bank.slot();
    let mut last_voted_fork_slots_aggregate = LastVotedForkSlotsAggregate::new(
        root_slot,
        REPAIR_THRESHOLD,
        root_bank.epoch_stakes(root_bank.epoch()).unwrap(),
        last_voted_fork_slots,
        &cluster_info.id(),
    );
    if let Some(aggregate_record) = &progress.last_voted_fork_slots_aggregate {
        for (key_string, message) in &aggregate_record.received {
            if let Err(e) =
                last_voted_fork_slots_aggregate.aggregate_from_record(key_string, message)
            {
                error!("Failed to aggregate from record: {:?}", e);
            }
        }
    } else {
        progress.last_voted_fork_slots_aggregate = Some(LastVotedForkSlotsAggregateRecord {
            received: HashMap::new(),
            final_result: None,
        });
    }
    let mut cursor = solana_gossip::crds::Cursor::default();
    let mut is_full_slots = HashSet::new();
    loop {
        if exit.load(Ordering::Relaxed) {
            return Err(WenRestartError::Exiting.into());
        }
        let start = timestamp();
        for new_last_voted_fork_slots in cluster_info.get_restart_last_voted_fork_slots(&mut cursor)
        {
            let from = new_last_voted_fork_slots.from.to_string();
            if let Some(record) =
                last_voted_fork_slots_aggregate.aggregate(new_last_voted_fork_slots)
            {
                progress
                    .last_voted_fork_slots_aggregate
                    .as_mut()
                    .unwrap()
                    .received
                    .insert(from, record);
            }
        }
        // Because all operations on the aggregate are called from this single thread, we can
        // fetch all results separately without worrying about them being out of sync. We can
        // also use returned iterator without the vector changing underneath us.
        let active_percent = last_voted_fork_slots_aggregate.active_percent();
        let mut filtered_slots: Vec<Slot>;
        {
            filtered_slots = last_voted_fork_slots_aggregate
                .slots_to_repair_iter()
                .filter(|slot| {
                    if *slot <= &root_slot || is_full_slots.contains(*slot) {
                        return false;
                    }
                    if blockstore.is_full(**slot) {
                        is_full_slots.insert(**slot);
                        false
                    } else {
                        true
                    }
                })
                .cloned()
                .collect();
        }
        filtered_slots.sort();
        info!(
            "Active peers: {} Slots to repair: {:?}",
            active_percent, &filtered_slots
        );
        if filtered_slots.is_empty()
            && active_percent >= wait_for_supermajority_threshold_percent as f64
        {
            *wen_restart_repair_slots.write().unwrap() = vec![];
            break;
        }
        {
            *wen_restart_repair_slots.write().unwrap() = filtered_slots;
        }
        write_wen_restart_records(wen_restart_path, progress)?;
        let elapsed = timestamp().saturating_sub(start);
        let time_left = GOSSIP_SLEEP_MILLIS.saturating_sub(elapsed);
        if time_left > 0 {
            sleep(Duration::from_millis(time_left));
        }
    }
    Ok(last_voted_fork_slots_aggregate.get_final_result())
}

// Verify that all blocks with at least (active_stake_percnet - 38%) of the stake form a
// single chain from the root, and use the highest slot in the blocks as the heaviest fork.
// Please see SIMD 46 "gossip current heaviest fork" for correctness proof.
pub(crate) fn find_heaviest_fork(
    aggregate_final_result: LastVotedForkSlotsFinalResult,
    bank_forks: Arc<RwLock<BankForks>>,
    blockstore: Arc<Blockstore>,
    exit: Arc<AtomicBool>,
) -> Result<(Slot, Hash)> {
    let root_bank = bank_forks.read().unwrap().root_bank();
    let root_slot = root_bank.slot();
    // TODO: Should use better epoch_stakes later.
    let epoch_stake = root_bank.epoch_stakes(root_bank.epoch()).unwrap();
    let total_stake = epoch_stake.total_stake();
    let stake_threshold = aggregate_final_result
        .total_active_stake
        .saturating_sub((HEAVIEST_FORK_THRESHOLD_DELTA * total_stake as f64) as u64);
    let mut slots = aggregate_final_result
        .slots_stake_map
        .iter()
        .filter(|(slot, stake)| **slot > root_slot && **stake > stake_threshold)
        .map(|(slot, _)| *slot)
        .collect::<Vec<Slot>>();
    slots.sort();

    // The heaviest slot we selected will always be the last of the slots list, or root if the list is empty.
    let heaviest_fork_slot = slots.last().map_or(root_slot, |x| *x);

    let mut expected_parent = root_slot;
    for slot in &slots {
        if exit.load(Ordering::Relaxed) {
            return Err(WenRestartError::Exiting.into());
        }
        if let Ok(Some(block_meta)) = blockstore.meta(*slot) {
            if block_meta.parent_slot != Some(expected_parent) {
                if expected_parent == root_slot {
                    error!("First block {} in repair list not linked to local root {}, this could mean our root is too old",
                        slot, root_slot);
                } else {
                    error!(
                        "Block {} in blockstore is not linked to expected parent from Wen Restart {} but to Block {:?}",
                        slot, expected_parent, block_meta.parent_slot
                    );
                }
                return Err(WenRestartError::BlockNotLinkedToExpectedParent(
                    *slot,
                    block_meta.parent_slot,
                    expected_parent,
                )
                .into());
            }
            if !block_meta.is_full() {
                return Err(WenRestartError::BlockNotFull(*slot).into());
            }
            expected_parent = *slot;
        } else {
            return Err(WenRestartError::BlockNotFound(*slot).into());
        }
    }
    let heaviest_fork_bankhash = find_bankhash_of_heaviest_fork(
        heaviest_fork_slot,
        slots,
        blockstore.clone(),
        bank_forks.clone(),
        root_bank,
        &exit,
    )?;
    info!(
        "Heaviest fork found: slot: {}, bankhash: {:?}",
        heaviest_fork_slot, heaviest_fork_bankhash
    );
    Ok((heaviest_fork_slot, heaviest_fork_bankhash))
}

fn check_slot_smaller_than_intended_snapshot_slot(
    slot: Slot,
    intended_snapshot_slot: Slot,
    directory: &Path,
) -> Result<()> {
    match slot.cmp(&intended_snapshot_slot) {
        std::cmp::Ordering::Greater => Err(WenRestartError::FutureSnapshotExists(
            intended_snapshot_slot,
            slot,
            directory.to_string_lossy().to_string(),
        )
        .into()),
        std::cmp::Ordering::Equal => Err(WenRestartError::GenerateSnapshotWhenOneExists(
            slot,
            directory.to_string_lossy().to_string(),
        )
        .into()),
        std::cmp::Ordering::Less => Ok(()),
    }
}

// Given the agreed upon slot, add hard fork and rehash the corresponding bank, then
// generate incremental snapshot. When the new snapshot is ready, it removes any
// incremental snapshot on the same slot, then moves the new snapshot into the
// incremental snapshot directory.
//
// We don't use set_root() explicitly, because it may kick off snapshot requests, we
// can't have multiple snapshot requests in progress. In bank_to_snapshot_archive()
// everything set_root() does will be done (without bank_forks setting root). So
// when we restart from the snapshot bank on new_root_slot will become root.
pub(crate) fn generate_snapshot(
    bank_forks: Arc<RwLock<BankForks>>,
    snapshot_config: &SnapshotConfig,
    accounts_background_request_sender: &AbsRequestSender,
    genesis_config_hash: Hash,
    new_root_slot: Slot,
) -> Result<GenerateSnapshotRecord> {
    let new_root_bank;
    {
        let mut my_bank_forks = bank_forks.write().unwrap();
        let old_root_bank = my_bank_forks.root_bank();
        if !old_root_bank
            .hard_forks()
            .iter()
            .any(|(slot, _)| slot == &new_root_slot)
        {
            old_root_bank.register_hard_fork(new_root_slot);
        }
        // new_root_slot is guaranteed to have a bank in bank_forks, it's checked in
        // find_bankhash_of_heaviest_fork().
        match my_bank_forks.get(new_root_slot) {
            Some(bank) => new_root_bank = bank.clone(),
            None => {
                return Err(WenRestartError::BlockNotFound(new_root_slot).into());
            }
        }
        let mut banks = vec![&new_root_bank];
        let parents = new_root_bank.parents();
        banks.extend(parents.iter());

        let _ = my_bank_forks.send_eah_request_if_needed(
            new_root_slot,
            &banks,
            accounts_background_request_sender,
        )?;
    }
    // There can't be more than one EAH calculation in progress. If new_root is generated
    // within the EAH window (1/4 epoch to 3/4 epoch), the following function will wait for
    // EAH calculation to finish. So if we trigger another EAH when generating snapshots
    // we won't hit a panic.
    let _ = new_root_bank.get_epoch_accounts_hash_to_serialize();
    // Even though generating incremental snapshot is faster, it involves finding a full
    // snapshot to use as base, so the logic is more complicated. For now we always generate
    // an incremental snapshot.
    let mut directory = &snapshot_config.full_snapshot_archives_dir;
    let Some(full_snapshot_slot) = get_highest_full_snapshot_archive_slot(directory) else {
        return Err(WenRestartError::MissingFullSnapshot(
            snapshot_config
                .full_snapshot_archives_dir
                .to_string_lossy()
                .to_string(),
        )
        .into());
    };
    // In very rare cases it's possible that the local root is not on the heaviest fork, so the
    // validator generated snapshot for slots > local root. If the cluster agreed upon restart
    // slot new_root_slot is less than the the current highest full_snapshot_slot, that means the
    // locally rooted full_snapshot_slot will be rolled back. this requires human inspection。
    //
    // In even rarer cases, the selected slot might be the latest full snapshot slot. We could
    // just re-generate a new snapshot to make sure the snapshot is up to date after hard fork,
    // but for now we just return an error to keep the code simple.
    check_slot_smaller_than_intended_snapshot_slot(full_snapshot_slot, new_root_slot, directory)?;
    directory = &snapshot_config.incremental_snapshot_archives_dir;
    if let Some(incremental_snapshot_slot) =
        get_highest_incremental_snapshot_archive_slot(directory, full_snapshot_slot)
    {
        check_slot_smaller_than_intended_snapshot_slot(
            incremental_snapshot_slot,
            new_root_slot,
            directory,
        )?;
    }
    let archive_info = bank_to_incremental_snapshot_archive(
        &snapshot_config.bank_snapshots_dir,
        &new_root_bank,
        full_snapshot_slot,
        Some(snapshot_config.snapshot_version),
        &snapshot_config.full_snapshot_archives_dir,
        &snapshot_config.incremental_snapshot_archives_dir,
        snapshot_config.archive_format,
    )?;
    let new_shred_version =
        compute_shred_version(&genesis_config_hash, Some(&new_root_bank.hard_forks()));
    let new_snapshot_path = archive_info.path().display().to_string();
    info!("wen_restart incremental snapshot generated on {new_snapshot_path} base slot {full_snapshot_slot}");
    // We might have bank snapshots past the new_root_slot, we need to purge them.
    purge_all_bank_snapshots(&snapshot_config.bank_snapshots_dir);
    Ok(GenerateSnapshotRecord {
        path: new_snapshot_path,
        slot: new_root_slot,
        bankhash: new_root_bank.hash().to_string(),
        shred_version: new_shred_version as u32,
    })
}

// Find the hash of the heaviest fork, if block hasn't been replayed, replay to get the hash.
pub(crate) fn find_bankhash_of_heaviest_fork(
    heaviest_fork_slot: Slot,
    slots: Vec<Slot>,
    blockstore: Arc<Blockstore>,
    bank_forks: Arc<RwLock<BankForks>>,
    root_bank: Arc<Bank>,
    exit: &AtomicBool,
) -> Result<Hash> {
    let heaviest_fork_bankhash = bank_forks
        .read()
        .unwrap()
        .get(heaviest_fork_slot)
        .map(|bank| bank.hash());
    if let Some(hash) = heaviest_fork_bankhash {
        return Ok(hash);
    }

    let leader_schedule_cache = LeaderScheduleCache::new_from_bank(&root_bank);
    let replay_tx_thread_pool = rayon::ThreadPoolBuilder::new()
        .thread_name(|i| format!("solReplayTx{i:02}"))
        .build()
        .expect("new rayon threadpool");
    let recyclers = VerifyRecyclers::default();
    let mut timing = ExecuteTimings::default();
    let opts = ProcessOptions::default();
    // Grab one write lock until end of function because we are the only one touching bankforks now.
    let mut my_bankforks = bank_forks.write().unwrap();
    // Now replay all the missing blocks.
    let mut parent_bank = root_bank;
    for slot in slots {
        if exit.load(Ordering::Relaxed) {
            return Err(WenRestartError::Exiting.into());
        }
        let bank = match my_bankforks.get(slot) {
            Some(cur_bank) => {
                if !cur_bank.is_frozen() {
                    return Err(WenRestartError::BlockNotFrozenAfterReplay(slot, None).into());
                }
                cur_bank
            }
            None => {
                let new_bank = Bank::new_from_parent(
                    parent_bank.clone(),
                    &leader_schedule_cache
                        .slot_leader_at(slot, Some(&parent_bank))
                        .unwrap(),
                    slot,
                );
                let bank_with_scheduler = my_bankforks.insert_from_ledger(new_bank);
                let mut progress = ConfirmationProgress::new(parent_bank.last_blockhash());
                if let Err(e) = process_single_slot(
                    &blockstore,
                    &bank_with_scheduler,
                    &replay_tx_thread_pool,
                    &opts,
                    &recyclers,
                    &mut progress,
                    None,
                    None,
                    None,
                    None,
                    &mut timing,
                ) {
                    return Err(WenRestartError::BlockNotFrozenAfterReplay(
                        slot,
                        Some(e.to_string()),
                    )
                    .into());
                }
                my_bankforks.get(slot).unwrap()
            }
        };
        parent_bank = bank;
    }
    Ok(parent_bank.hash())
}

// Aggregate the heaviest fork and send updates to the cluster.
pub(crate) fn aggregate_restart_heaviest_fork(
    wen_restart_path: &PathBuf,
    wait_for_supermajority_threshold_percent: u64,
    cluster_info: Arc<ClusterInfo>,
    bank_forks: Arc<RwLock<BankForks>>,
    exit: Arc<AtomicBool>,
    progress: &mut WenRestartProgress,
) -> Result<()> {
    let root_bank = bank_forks.read().unwrap().root_bank();
    let epoch_stakes = root_bank.epoch_stakes(root_bank.epoch()).unwrap();
    let total_stake = epoch_stakes.total_stake();
    if progress.my_heaviest_fork.is_none() {
        return Err(WenRestartError::MalformedProgress(
            RestartState::HeaviestFork,
            "my_heaviest_fork".to_string(),
        )
        .into());
    }
    let my_heaviest_fork = progress.my_heaviest_fork.clone().unwrap();
    let heaviest_fork_slot = my_heaviest_fork.slot;
    let heaviest_fork_hash = Hash::from_str(&my_heaviest_fork.bankhash)?;
    let adjusted_threshold_percent = wait_for_supermajority_threshold_percent
        .saturating_sub(HEAVIEST_FORK_DISAGREE_THRESHOLD_PERCENT.round() as u64);
    // The threshold for supermajority should definitely be higher than 67%.
    assert!(
        adjusted_threshold_percent > 67,
        "Majority threshold too low"
    );
    let mut heaviest_fork_aggregate = HeaviestForkAggregate::new(
        adjusted_threshold_percent,
        cluster_info.my_shred_version(),
        epoch_stakes,
        heaviest_fork_slot,
        heaviest_fork_hash,
        &cluster_info.id(),
    );
    if let Some(aggregate_record) = &progress.heaviest_fork_aggregate {
        for (key_string, message) in &aggregate_record.received {
            match heaviest_fork_aggregate.aggregate_from_record(key_string, message) {
                Err(e) => error!("Failed to aggregate from record: {:?}", e),
                Ok(None) => info!("Record {:?} ignored", message),
                Ok(_) => (),
            }
        }
    } else {
        progress.heaviest_fork_aggregate = Some(HeaviestForkAggregateRecord {
            received: HashMap::new(),
            final_result: None,
        });
    }

    let mut total_active_stake = heaviest_fork_aggregate.total_active_stake();
    progress
        .my_heaviest_fork
        .as_mut()
        .unwrap()
        .total_active_stake = total_active_stake;

    let mut progress_last_sent = Instant::now();
    let mut cursor = solana_gossip::crds::Cursor::default();
    let mut progress_changed = false;
    let majority_stake_required =
        (total_stake as f64 / 100.0 * adjusted_threshold_percent as f64).round() as u64;
    let mut total_active_stake_higher_than_supermajority = false;
    let mut first_time_entering_loop = true;
    loop {
        if exit.load(Ordering::Relaxed) {
            return Err(WenRestartError::Exiting.into());
        }
        let start = timestamp();
        for new_heaviest_fork in cluster_info.get_restart_heaviest_fork(&mut cursor) {
            info!("Received new heaviest fork: {:?}", new_heaviest_fork);
            let from = new_heaviest_fork.from.to_string();
            if let Some(record) = heaviest_fork_aggregate.aggregate(new_heaviest_fork) {
                info!("Successfully aggregated new heaviest fork: {:?}", record);
                progress
                    .heaviest_fork_aggregate
                    .as_mut()
                    .unwrap()
                    .received
                    .insert(from, record);
                progress_changed = true;
            }
        }
        let current_total_active_stake = heaviest_fork_aggregate.total_active_stake();
        if current_total_active_stake > total_active_stake {
            total_active_stake = current_total_active_stake;
            progress
                .my_heaviest_fork
                .as_mut()
                .unwrap()
                .total_active_stake = current_total_active_stake;
            progress_changed = true;
        }
        if progress_changed {
            progress_changed = false;
            let total_active_stake_seen_supermajority =
                heaviest_fork_aggregate.total_active_stake_seen_supermajority();
            info!(
                "Total active stake seeing supermajority: {} Total active stake: {} Total stake {}",
                total_active_stake_seen_supermajority,
                heaviest_fork_aggregate.total_active_stake(),
                total_stake
            );
            let can_exit = total_active_stake_seen_supermajority >= majority_stake_required;
            let saw_supermajority_first_time = current_total_active_stake
                >= majority_stake_required
                && !total_active_stake_higher_than_supermajority
                && {
                    total_active_stake_higher_than_supermajority = true;
                    true
                };
            // Only send out updates every 5 minutes or when we can exit or active stake passes supermajority
            // the first time.
            if progress_last_sent.elapsed().as_secs() >= HEAVIEST_REFRESH_INTERVAL_IN_SECONDS
                || can_exit
                || first_time_entering_loop
                || saw_supermajority_first_time
            {
                first_time_entering_loop = false;
                cluster_info.push_restart_heaviest_fork(
                    heaviest_fork_slot,
                    heaviest_fork_hash,
                    current_total_active_stake,
                );
                write_wen_restart_records(wen_restart_path, progress)?;
                progress_last_sent = Instant::now();
            }
            if can_exit {
                break;
            }
        }
        let elapsed = timestamp().saturating_sub(start);
        let time_left = GOSSIP_SLEEP_MILLIS.saturating_sub(elapsed);
        if time_left > 0 {
            sleep(Duration::from_millis(time_left));
        }
    }

    // Final check to see if supermajority agrees with us.
    let total_active_stake = heaviest_fork_aggregate.total_active_stake();
    let total_active_stake_seen_supermajority =
        heaviest_fork_aggregate.total_active_stake_seen_supermajority();
    let block_stake_map = heaviest_fork_aggregate.block_stake_map();
    let total_active_stake_agreed_with_me = *block_stake_map
        .get(&(heaviest_fork_slot, heaviest_fork_hash))
        .unwrap_or(&0);
    // It doesn't matter if 5% disagrees with us.
    let success_threshold =
        wait_for_supermajority_threshold_percent as f64 - HEAVIEST_FORK_DISAGREE_THRESHOLD_PERCENT;
    if total_active_stake_agreed_with_me as f64 * 100.0 / total_stake as f64 >= success_threshold {
        info!(
            "Heaviest fork agreed upon by supermajority: slot: {}, bankhash: {}",
            heaviest_fork_slot, heaviest_fork_hash
        );
        progress
            .heaviest_fork_aggregate
            .as_mut()
            .unwrap()
            .final_result = Some(HeaviestForkAggregateFinal {
            total_active_stake,
            total_active_stake_seen_supermajority,
            total_active_stake_agreed_with_me,
        });
        Ok(())
    } else {
        info!(
            "Not enough stake agreeing with our heaviest fork: slot: {},
            bankhash: {}, stake aggreeing with us {} out of {}",
            heaviest_fork_slot,
            heaviest_fork_hash,
            total_active_stake_agreed_with_me,
            total_active_stake
        );
        let mut max_slot_hash = (0, Hash::default());
        let mut max_stake = 0;
        for (slot, hash) in block_stake_map.keys() {
            let stake = block_stake_map[&(*slot, *hash)];
            if stake > max_stake {
                max_stake = stake;
                max_slot_hash = (*slot, *hash);
            }
            info!(
                "Slot: {}, Hash: {}, Stake: {}",
                slot,
                hash,
                block_stake_map[&(*slot, *hash)]
            );
        }
        let max_stake_percent = max_stake as f64 * 100.0 / total_stake as f64;
        if max_stake_percent >= success_threshold {
            warn!(
                "Max stake slot: {}, hash: {}, stake: {:.2}% does not agree with my
                choice, please go to discord to download the snapshot and restart
                the validator with --wait-for-supermajority.",
                max_slot_hash.0, max_slot_hash.1, max_stake_percent
            );
        } else {
            warn!(
                "Cluster consensus slot: {}, hash: {}, stake: {:.2}% does not agree,
                please go to discord for next steps.",
                max_slot_hash.0, max_slot_hash.1, max_stake_percent
            );
        }
        Err(WenRestartError::NotEnoughStakeAgreeingWithUs(
            heaviest_fork_slot,
            heaviest_fork_hash,
            block_stake_map,
        )
        .into())
    }
}

pub struct WenRestartConfig {
    pub wen_restart_path: PathBuf,
    pub last_vote: VoteTransaction,
    pub blockstore: Arc<Blockstore>,
    pub cluster_info: Arc<ClusterInfo>,
    pub bank_forks: Arc<RwLock<BankForks>>,
    pub wen_restart_repair_slots: Option<Arc<RwLock<Vec<Slot>>>>,
    pub wait_for_supermajority_threshold_percent: u64,
    pub snapshot_config: SnapshotConfig,
    pub accounts_background_request_sender: AbsRequestSender,
    pub genesis_config_hash: Hash,
    pub exit: Arc<AtomicBool>,
}

pub fn wait_for_wen_restart(config: WenRestartConfig) -> Result<()> {
    let (mut state, mut progress) = initialize(
        &config.wen_restart_path,
        config.last_vote.clone(),
        config.blockstore.clone(),
    )?;
    loop {
        state = match state {
            WenRestartProgressInternalState::Init {
                last_voted_fork_slots,
                last_vote_bankhash,
            } => {
                progress.my_last_voted_fork_slots = Some(send_restart_last_voted_fork_slots(
                    config.cluster_info.clone(),
                    &last_voted_fork_slots,
                    last_vote_bankhash,
                )?);
                WenRestartProgressInternalState::Init {
                    last_voted_fork_slots,
                    last_vote_bankhash,
                }
            }
            WenRestartProgressInternalState::LastVotedForkSlots {
                last_voted_fork_slots,
                aggregate_final_result,
            } => {
                let final_result = match aggregate_final_result {
                    Some(result) => result,
                    None => aggregate_restart_last_voted_fork_slots(
                        &config.wen_restart_path,
                        config.wait_for_supermajority_threshold_percent,
                        config.cluster_info.clone(),
                        &last_voted_fork_slots,
                        config.bank_forks.clone(),
                        config.blockstore.clone(),
                        config.wen_restart_repair_slots.clone().unwrap(),
                        config.exit.clone(),
                        &mut progress,
                    )?,
                };
                WenRestartProgressInternalState::LastVotedForkSlots {
                    last_voted_fork_slots,
                    aggregate_final_result: Some(final_result),
                }
            }
            WenRestartProgressInternalState::FindHeaviestFork {
                aggregate_final_result,
                my_heaviest_fork,
            } => {
                let heaviest_fork = match my_heaviest_fork {
                    Some(heaviest_fork) => heaviest_fork,
                    None => {
                        let (slot, bankhash) = find_heaviest_fork(
                            aggregate_final_result.clone(),
                            config.bank_forks.clone(),
                            config.blockstore.clone(),
                            config.exit.clone(),
                        )?;
                        info!(
                            "Heaviest fork found: slot: {}, bankhash: {}",
                            slot, bankhash
                        );
                        HeaviestForkRecord {
                            slot,
                            bankhash: bankhash.to_string(),
                            total_active_stake: 0,
                            wallclock: 0,
                            shred_version: config.cluster_info.my_shred_version() as u32,
                        }
                    }
                };
                WenRestartProgressInternalState::FindHeaviestFork {
                    aggregate_final_result,
                    my_heaviest_fork: Some(heaviest_fork),
                }
            }
            WenRestartProgressInternalState::HeaviestFork { new_root_slot } => {
                aggregate_restart_heaviest_fork(
                    &config.wen_restart_path,
                    config.wait_for_supermajority_threshold_percent,
                    config.cluster_info.clone(),
                    config.bank_forks.clone(),
                    config.exit.clone(),
                    &mut progress,
                )?;
                WenRestartProgressInternalState::HeaviestFork { new_root_slot }
            }
            WenRestartProgressInternalState::GenerateSnapshot {
                new_root_slot,
                my_snapshot,
            } => {
                let snapshot_record = match my_snapshot {
                    Some(record) => record,
                    None => generate_snapshot(
                        config.bank_forks.clone(),
                        &config.snapshot_config,
                        &config.accounts_background_request_sender,
                        config.genesis_config_hash,
                        new_root_slot,
                    )?,
                };
                WenRestartProgressInternalState::GenerateSnapshot {
                    new_root_slot,
                    my_snapshot: Some(snapshot_record),
                }
            }
            WenRestartProgressInternalState::Done => return Ok(()),
        };
        state = increment_and_write_wen_restart_records(
            &config.wen_restart_path,
            state,
            &mut progress,
        )?;
    }
}

pub(crate) fn increment_and_write_wen_restart_records(
    records_path: &PathBuf,
    current_state: WenRestartProgressInternalState,
    progress: &mut WenRestartProgress,
) -> Result<WenRestartProgressInternalState> {
    let new_state = match current_state {
        WenRestartProgressInternalState::Init {
            last_voted_fork_slots,
            last_vote_bankhash: _,
        } => {
            progress.set_state(RestartState::LastVotedForkSlots);
            WenRestartProgressInternalState::LastVotedForkSlots {
                last_voted_fork_slots,
                aggregate_final_result: None,
            }
        }
        WenRestartProgressInternalState::LastVotedForkSlots {
            last_voted_fork_slots: _,
            aggregate_final_result,
        } => {
            if let Some(aggregate_final_result) = aggregate_final_result {
                progress.set_state(RestartState::HeaviestFork);
                if let Some(aggregate_record) = progress.last_voted_fork_slots_aggregate.as_mut() {
                    aggregate_record.final_result = Some(LastVotedForkSlotsAggregateFinal {
                        slots_stake_map: aggregate_final_result.slots_stake_map.clone(),
                        total_active_stake: aggregate_final_result.total_active_stake,
                    });
                }
                WenRestartProgressInternalState::FindHeaviestFork {
                    aggregate_final_result,
                    my_heaviest_fork: None,
                }
            } else {
                return Err(
                    WenRestartError::UnexpectedState(RestartState::LastVotedForkSlots).into(),
                );
            }
        }
        WenRestartProgressInternalState::FindHeaviestFork {
            aggregate_final_result: _,
            my_heaviest_fork,
        } => {
            if let Some(my_heaviest_fork) = my_heaviest_fork {
                progress.my_heaviest_fork = Some(my_heaviest_fork.clone());
                WenRestartProgressInternalState::HeaviestFork {
                    new_root_slot: my_heaviest_fork.slot,
                }
            } else {
                return Err(WenRestartError::UnexpectedState(RestartState::HeaviestFork).into());
            }
        }
        WenRestartProgressInternalState::HeaviestFork { new_root_slot } => {
            progress.set_state(RestartState::GenerateSnapshot);
            WenRestartProgressInternalState::GenerateSnapshot {
                new_root_slot,
                my_snapshot: None,
            }
        }
        WenRestartProgressInternalState::GenerateSnapshot {
            new_root_slot: _,
            my_snapshot,
        } => {
            if let Some(my_snapshot) = my_snapshot {
                progress.set_state(RestartState::Done);
                progress.my_snapshot = Some(my_snapshot.clone());
                WenRestartProgressInternalState::Done
            } else {
                return Err(WenRestartError::UnexpectedState(RestartState::Done).into());
            }
        }
        WenRestartProgressInternalState::Done => {
            return Err(WenRestartError::UnexpectedState(RestartState::Done).into())
        }
    };
    write_wen_restart_records(records_path, progress)?;
    Ok(new_state)
}

pub(crate) fn initialize(
    records_path: &PathBuf,
    last_vote: VoteTransaction,
    blockstore: Arc<Blockstore>,
) -> Result<(WenRestartProgressInternalState, WenRestartProgress)> {
    let progress = match read_wen_restart_records(records_path) {
        Ok(progress) => progress,
        Err(e) => {
            let stdio_err = e.downcast_ref::<std::io::Error>();
            if stdio_err.is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
                info!(
                    "wen restart proto file not found at {:?}, write init state",
                    records_path
                );
                let progress = WenRestartProgress {
                    state: RestartState::Init.into(),
                    ..Default::default()
                };
                write_wen_restart_records(records_path, &progress)?;
                progress
            } else {
                return Err(e);
            }
        }
    };
    match progress.state() {
        RestartState::Done => Ok((WenRestartProgressInternalState::Done, progress)),
        RestartState::Init => {
            let last_voted_fork_slots;
            let last_vote_bankhash;
            match &progress.my_last_voted_fork_slots {
                Some(my_last_voted_fork_slots) => {
                    last_voted_fork_slots = my_last_voted_fork_slots.last_voted_fork_slots.clone();
                    last_vote_bankhash =
                        Hash::from_str(&my_last_voted_fork_slots.last_vote_bankhash).unwrap();
                }
                None => {
                    // repair and restart option does not work without last voted slot.
                    if let Some(last_vote_slot) = last_vote.last_voted_slot() {
                        last_vote_bankhash = last_vote.hash();
                        last_voted_fork_slots =
                            AncestorIterator::new_inclusive(last_vote_slot, &blockstore)
                                .take(RestartLastVotedForkSlots::MAX_SLOTS)
                                .collect();
                    } else {
                        error!("
                            Cannot find last voted slot in the tower storage, it either means that this node has never \
                            voted or the tower storage is corrupted. Unfortunately, since WenRestart is a consensus protocol \
                            depending on each participant to send their last voted fork slots, your validator cannot participate.\
                            Please check discord for the conclusion of the WenRestart protocol, then generate a snapshot and use \
                            --wait-for-supermajority to restart the validator.");
                        return Err(WenRestartError::MissingLastVotedForkSlots.into());
                    }
                }
            }
            Ok((
                WenRestartProgressInternalState::Init {
                    last_voted_fork_slots,
                    last_vote_bankhash,
                },
                progress,
            ))
        }
        RestartState::LastVotedForkSlots => {
            if let Some(record) = progress.my_last_voted_fork_slots.as_ref() {
                Ok((
                    WenRestartProgressInternalState::LastVotedForkSlots {
                        last_voted_fork_slots: record.last_voted_fork_slots.clone(),
                        aggregate_final_result: progress
                            .last_voted_fork_slots_aggregate
                            .as_ref()
                            .and_then(|r| {
                                r.final_result.as_ref().map(|result| {
                                    LastVotedForkSlotsFinalResult {
                                        slots_stake_map: result.slots_stake_map.clone(),
                                        total_active_stake: result.total_active_stake,
                                    }
                                })
                            }),
                    },
                    progress,
                ))
            } else {
                Err(WenRestartError::MalformedLastVotedForkSlotsProtobuf(None).into())
            }
        }
        RestartState::HeaviestFork => Ok((
            WenRestartProgressInternalState::FindHeaviestFork {
                aggregate_final_result: progress
                    .last_voted_fork_slots_aggregate
                    .as_ref()
                    .and_then(|r| {
                        r.final_result
                            .as_ref()
                            .map(|result| LastVotedForkSlotsFinalResult {
                                slots_stake_map: result.slots_stake_map.clone(),
                                total_active_stake: result.total_active_stake,
                            })
                    })
                    .ok_or(WenRestartError::MalformedProgress(
                        RestartState::HeaviestFork,
                        "final_result in last_voted_fork_slots_aggregate".to_string(),
                    ))?,
                my_heaviest_fork: progress.my_heaviest_fork.clone(),
            },
            progress,
        )),
        RestartState::GenerateSnapshot => Ok((
            WenRestartProgressInternalState::GenerateSnapshot {
                new_root_slot: progress
                    .my_heaviest_fork
                    .as_ref()
                    .ok_or(WenRestartError::MalformedProgress(
                        RestartState::GenerateSnapshot,
                        "my_heaviest_fork".to_string(),
                    ))?
                    .slot,
                my_snapshot: progress.my_snapshot.clone(),
            },
            progress,
        )),
    }
}

fn read_wen_restart_records(records_path: &PathBuf) -> Result<WenRestartProgress> {
    let buffer = read(records_path)?;
    let progress = WenRestartProgress::decode(&mut Cursor::new(buffer))?;
    info!("read record {:?}", progress);
    Ok(progress)
}

pub(crate) fn write_wen_restart_records(
    records_path: &PathBuf,
    new_progress: &WenRestartProgress,
) -> Result<()> {
    // overwrite anything if exists
    let mut file = File::create(records_path)?;
    info!("writing new record {:?}", new_progress);
    let mut buf = Vec::with_capacity(new_progress.encoded_len());
    new_progress.encode(&mut buf)?;
    file.write_all(&buf)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        crate::wen_restart::{tests::wen_restart_proto::LastVotedForkSlotsAggregateFinal, *},
        solana_accounts_db::hardened_unpack::MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
        solana_entry::entry::create_ticks,
        solana_gossip::{
            cluster_info::ClusterInfo,
            contact_info::ContactInfo,
            crds::GossipRoute,
            crds_value::{CrdsData, CrdsValue},
            restart_crds_values::{RestartHeaviestFork, RestartLastVotedForkSlots},
        },
        solana_ledger::{
            blockstore::{create_new_ledger, entries_to_test_shreds, Blockstore},
            blockstore_options::LedgerColumnOptions,
            blockstore_processor::{fill_blockstore_slot_with_ticks, test_process_blockstore},
            get_tmp_ledger_path_auto_delete,
        },
        solana_program::{
            hash::Hash,
            vote::state::{TowerSync, Vote},
        },
        solana_runtime::{
            genesis_utils::{
                create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
            },
            snapshot_bank_utils::bank_to_full_snapshot_archive,
            snapshot_hash::SnapshotHash,
            snapshot_utils::build_incremental_snapshot_archive_path,
        },
        solana_sdk::{
            signature::{Keypair, Signer},
            timing::timestamp,
        },
        solana_streamer::socket::SocketAddrSpace,
        std::{fs::remove_file, sync::Arc, thread::Builder},
        tempfile::TempDir,
    };

    const SHRED_VERSION: u16 = 2;
    const EXPECTED_SLOTS: Slot = 90;
    const TICKS_PER_SLOT: u64 = 2;
    const TOTAL_VALIDATOR_COUNT: u16 = 20;
    const MY_INDEX: usize = TOTAL_VALIDATOR_COUNT as usize - 1;
    const WAIT_FOR_THREAD_TIMEOUT: u64 = 10_000;
    const WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT: u64 = 80;
    const NON_CONFORMING_VALIDATOR_PERCENT: u64 = 5;

    fn push_restart_last_voted_fork_slots(
        cluster_info: Arc<ClusterInfo>,
        node: &ContactInfo,
        last_voted_fork_slots: &[Slot],
        last_vote_hash: &Hash,
        node_keypair: &Keypair,
        wallclock: u64,
    ) {
        let slots = RestartLastVotedForkSlots::new(
            *node.pubkey(),
            wallclock,
            last_voted_fork_slots,
            *last_vote_hash,
            SHRED_VERSION,
        )
        .unwrap();
        let entries = vec![
            CrdsValue::new_signed(CrdsData::ContactInfo(node.clone()), node_keypair),
            CrdsValue::new_signed(CrdsData::RestartLastVotedForkSlots(slots), node_keypair),
        ];
        {
            let mut gossip_crds = cluster_info.gossip.crds.write().unwrap();
            for entry in entries {
                assert!(gossip_crds
                    .insert(entry, /*now=*/ 0, GossipRoute::LocalMessage)
                    .is_ok());
            }
        }
    }

    fn push_restart_heaviest_fork(
        cluster_info: Arc<ClusterInfo>,
        node: &ContactInfo,
        heaviest_fork_slot: Slot,
        heaviest_fork_hash: &Hash,
        observed_stake: u64,
        node_keypair: &Keypair,
        wallclock: u64,
    ) {
        let heaviest_fork = RestartHeaviestFork {
            from: *node.pubkey(),
            wallclock,
            last_slot: heaviest_fork_slot,
            last_slot_hash: *heaviest_fork_hash,
            observed_stake,
            shred_version: SHRED_VERSION,
        };
        assert!(cluster_info
            .gossip
            .crds
            .write()
            .unwrap()
            .insert(
                CrdsValue::new_signed(CrdsData::RestartHeaviestFork(heaviest_fork), node_keypair),
                /*now=*/ 0,
                GossipRoute::LocalMessage
            )
            .is_ok());
    }

    struct WenRestartTestInitResult {
        pub validator_voting_keypairs: Vec<ValidatorVoteKeypairs>,
        pub blockstore: Arc<Blockstore>,
        pub cluster_info: Arc<ClusterInfo>,
        pub bank_forks: Arc<RwLock<BankForks>>,
        pub last_voted_fork_slots: Vec<Slot>,
        pub wen_restart_proto_path: PathBuf,
        pub last_blockhash: Hash,
        pub genesis_config_hash: Hash,
    }

    fn insert_slots_into_blockstore(
        blockstore: Arc<Blockstore>,
        first_parent: Slot,
        slots_to_insert: &[Slot],
        entries_per_slot: u64,
        start_blockhash: Hash,
    ) -> Hash {
        let mut last_hash = start_blockhash;
        let mut last_parent = first_parent;
        for i in slots_to_insert {
            last_hash = fill_blockstore_slot_with_ticks(
                &blockstore,
                entries_per_slot,
                *i,
                last_parent,
                last_hash,
            );
            last_parent = *i;
        }
        last_hash
    }

    fn wen_restart_test_init(ledger_path: &TempDir) -> WenRestartTestInitResult {
        let validator_voting_keypairs: Vec<_> = (0..TOTAL_VALIDATOR_COUNT)
            .map(|_| ValidatorVoteKeypairs::new_rand())
            .collect();
        let node_keypair = Arc::new(
            validator_voting_keypairs[MY_INDEX]
                .node_keypair
                .insecure_clone(),
        );
        let cluster_info = Arc::new(ClusterInfo::new(
            {
                let mut contact_info =
                    ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp());
                contact_info.set_shred_version(SHRED_VERSION);
                contact_info
            },
            node_keypair.clone(),
            SocketAddrSpace::Unspecified,
        ));
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config_with_vote_accounts(
            10_000,
            &validator_voting_keypairs,
            vec![100; validator_voting_keypairs.len()],
        );
        genesis_config.ticks_per_slot = TICKS_PER_SLOT;
        let start_blockhash = create_new_ledger(
            ledger_path.path(),
            &genesis_config,
            MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
            LedgerColumnOptions::default(),
        )
        .unwrap();
        let blockstore = Arc::new(Blockstore::open(ledger_path.path()).unwrap());
        let (bank_forks, ..) = test_process_blockstore(
            &genesis_config,
            &blockstore,
            &ProcessOptions {
                run_verification: true,
                accounts_db_test_hash_calculation: true,
                ..ProcessOptions::default()
            },
            Arc::default(),
        );
        let mut last_blockhash = start_blockhash;
        // Skip block 1, 2 links directly to 0.
        let last_parent: Slot = 2;
        let mut last_voted_fork_slots: Vec<Slot> = Vec::new();
        last_voted_fork_slots
            .extend(last_parent..last_parent.saturating_add(EXPECTED_SLOTS).saturating_add(1));
        last_blockhash = insert_slots_into_blockstore(
            blockstore.clone(),
            0,
            &last_voted_fork_slots,
            genesis_config.ticks_per_slot,
            last_blockhash,
        );
        last_voted_fork_slots.insert(0, 0);
        last_voted_fork_slots.reverse();
        let mut wen_restart_proto_path = ledger_path.path().to_path_buf();
        wen_restart_proto_path.push("wen_restart_status.proto");
        let _ = remove_file(&wen_restart_proto_path);
        WenRestartTestInitResult {
            validator_voting_keypairs,
            blockstore,
            cluster_info,
            bank_forks,
            last_voted_fork_slots,
            wen_restart_proto_path,
            last_blockhash,
            genesis_config_hash: genesis_config.hash(),
        }
    }

    fn wait_on_expected_progress_with_timeout(
        wen_restart_proto_path: PathBuf,
        expected_progress: WenRestartProgress,
    ) {
        let start = timestamp();
        let mut progress = WenRestartProgress {
            state: RestartState::Init.into(),
            ..Default::default()
        };
        loop {
            if let Ok(new_progress) = read_wen_restart_records(&wen_restart_proto_path) {
                progress = new_progress;
                if let Some(my_last_voted_fork_slots) = &expected_progress.my_last_voted_fork_slots
                {
                    if let Some(record) = progress.my_last_voted_fork_slots.as_mut() {
                        record.wallclock = my_last_voted_fork_slots.wallclock;
                    }
                }
                if progress == expected_progress {
                    return;
                }
            }
            if timestamp().saturating_sub(start) > WAIT_FOR_THREAD_TIMEOUT {
                assert_eq!(
                    progress.my_last_voted_fork_slots,
                    expected_progress.my_last_voted_fork_slots
                );
                assert_eq!(
                    progress.last_voted_fork_slots_aggregate,
                    expected_progress.last_voted_fork_slots_aggregate
                );
                panic!(
                    "wait_on_expected_progress_with_timeout failed to get expected progress {:?} expected {:?}",
                    &progress,
                    expected_progress
                );
            }
            sleep(Duration::from_millis(10));
        }
    }

    fn wen_restart_test_succeed_after_failure(
        test_state: WenRestartTestInitResult,
        last_vote_bankhash: Hash,
        expected_progress: WenRestartProgress,
    ) {
        // continue normally after the error, we should be good.
        let exit = Arc::new(AtomicBool::new(false));
        let last_vote_slot: Slot = test_state.last_voted_fork_slots[0];
        let wen_restart_config = WenRestartConfig {
            wen_restart_path: test_state.wen_restart_proto_path.clone(),
            last_vote: VoteTransaction::from(Vote::new(vec![last_vote_slot], last_vote_bankhash)),
            blockstore: test_state.blockstore.clone(),
            cluster_info: test_state.cluster_info.clone(),
            bank_forks: test_state.bank_forks.clone(),
            wen_restart_repair_slots: Some(Arc::new(RwLock::new(Vec::new()))),
            wait_for_supermajority_threshold_percent: 80,
            snapshot_config: SnapshotConfig::default(),
            accounts_background_request_sender: AbsRequestSender::default(),
            genesis_config_hash: test_state.genesis_config_hash,
            exit: exit.clone(),
        };
        let wen_restart_thread_handle = Builder::new()
            .name("solana-wen-restart".to_string())
            .spawn(move || {
                let _ = wait_for_wen_restart(wen_restart_config).is_ok();
            })
            .unwrap();
        wait_on_expected_progress_with_timeout(
            test_state.wen_restart_proto_path.clone(),
            expected_progress,
        );
        exit.store(true, Ordering::Relaxed);
        assert!(wen_restart_thread_handle.join().is_ok());
        let _ = remove_file(&test_state.wen_restart_proto_path);
    }

    #[test]
    fn test_wen_restart_normal_flow() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let wen_restart_repair_slots = Some(Arc::new(RwLock::new(Vec::new())));
        let test_state = wen_restart_test_init(&ledger_path);
        let last_vote_slot = test_state.last_voted_fork_slots[0];
        let last_vote_bankhash = Hash::new_unique();
        let expected_slots_to_repair: Vec<Slot> =
            (last_vote_slot + 1..last_vote_slot + 3).collect();

        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let snapshot_config = SnapshotConfig {
            bank_snapshots_dir: bank_snapshots_dir.as_ref().to_path_buf(),
            full_snapshot_archives_dir: full_snapshot_archives_dir.as_ref().to_path_buf(),
            incremental_snapshot_archives_dir: incremental_snapshot_archives_dir
                .as_ref()
                .to_path_buf(),
            ..Default::default()
        };
        test_state
            .bank_forks
            .write()
            .unwrap()
            .set_snapshot_config(Some(snapshot_config.clone()));
        let old_root_bank = test_state.bank_forks.read().unwrap().root_bank();
        // Trigger full snapshot generation on the old root bank.
        assert!(bank_to_full_snapshot_archive(
            snapshot_config.bank_snapshots_dir.clone(),
            &old_root_bank,
            Some(snapshot_config.snapshot_version),
            snapshot_config.full_snapshot_archives_dir.clone(),
            snapshot_config.incremental_snapshot_archives_dir.clone(),
            snapshot_config.archive_format,
        )
        .is_ok());

        let exit = Arc::new(AtomicBool::new(false));
        let wen_restart_config = WenRestartConfig {
            wen_restart_path: test_state.wen_restart_proto_path.clone(),
            last_vote: VoteTransaction::from(Vote::new(vec![last_vote_slot], last_vote_bankhash)),
            blockstore: test_state.blockstore.clone(),
            cluster_info: test_state.cluster_info.clone(),
            bank_forks: test_state.bank_forks.clone(),
            wen_restart_repair_slots: wen_restart_repair_slots.clone(),
            wait_for_supermajority_threshold_percent: 80,
            snapshot_config,
            accounts_background_request_sender: AbsRequestSender::default(),
            genesis_config_hash: test_state.genesis_config_hash,
            exit: exit.clone(),
        };
        let wen_restart_thread_handle = Builder::new()
            .name("solana-wen-restart".to_string())
            .spawn(move || {
                assert!(wait_for_wen_restart(wen_restart_config).is_ok());
            })
            .unwrap();
        let mut rng = rand::thread_rng();
        let mut expected_received_last_voted_fork_slots = HashMap::new();
        let validators_to_take: usize =
            (WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT * TOTAL_VALIDATOR_COUNT as u64 / 100 - 1)
                .try_into()
                .unwrap();
        let mut last_voted_fork_slots_from_others = test_state.last_voted_fork_slots.clone();
        last_voted_fork_slots_from_others.reverse();
        last_voted_fork_slots_from_others.append(&mut expected_slots_to_repair.clone());
        for keypairs in test_state
            .validator_voting_keypairs
            .iter()
            .take(validators_to_take)
        {
            let node_pubkey = keypairs.node_keypair.pubkey();
            let node = ContactInfo::new_rand(&mut rng, Some(node_pubkey));
            let last_vote_hash = Hash::new_unique();
            let now = timestamp();
            push_restart_last_voted_fork_slots(
                test_state.cluster_info.clone(),
                &node,
                &last_voted_fork_slots_from_others,
                &last_vote_hash,
                &keypairs.node_keypair,
                now,
            );
            expected_received_last_voted_fork_slots.insert(
                node_pubkey.to_string(),
                LastVotedForkSlotsRecord {
                    last_voted_fork_slots: last_voted_fork_slots_from_others.clone(),
                    last_vote_bankhash: last_vote_hash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: now,
                },
            );
        }

        // Simulating successful repair of missing blocks.
        let _ = insert_slots_into_blockstore(
            test_state.blockstore.clone(),
            last_vote_slot,
            &expected_slots_to_repair,
            TICKS_PER_SLOT,
            test_state.last_blockhash,
        );

        let expected_heaviest_fork_slot = last_vote_slot + 2;
        let expected_heaviest_fork_bankhash;
        loop {
            if let Some(bank) = test_state
                .bank_forks
                .read()
                .unwrap()
                .get(expected_heaviest_fork_slot)
            {
                expected_heaviest_fork_bankhash = bank.hash();
                break;
            }
            sleep(Duration::from_millis(100));
        }
        // Now simulate receiving HeaviestFork messages.
        let mut expected_received_heaviest_fork = HashMap::new();
        let validators_to_take: usize = ((WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT
            - NON_CONFORMING_VALIDATOR_PERCENT)
            * TOTAL_VALIDATOR_COUNT as u64
            / 100
            - 1)
        .try_into()
        .unwrap();
        // HeaviestFork only requires 75% vs 80% required for LastVotedForkSlots. We have 5% stake, so we need 70%.
        let total_active_stake_during_heaviest_fork = (validators_to_take + 1) as u64 * 100;
        for keypairs in test_state
            .validator_voting_keypairs
            .iter()
            .take(validators_to_take)
        {
            let node_pubkey = keypairs.node_keypair.pubkey();
            let node = ContactInfo::new_rand(&mut rng, Some(node_pubkey));
            let now = timestamp();
            push_restart_heaviest_fork(
                test_state.cluster_info.clone(),
                &node,
                expected_heaviest_fork_slot,
                &expected_heaviest_fork_bankhash,
                total_active_stake_during_heaviest_fork,
                &keypairs.node_keypair,
                now,
            );
            expected_received_heaviest_fork.insert(
                node_pubkey.to_string(),
                HeaviestForkRecord {
                    slot: expected_heaviest_fork_slot,
                    bankhash: expected_heaviest_fork_bankhash.to_string(),
                    total_active_stake: total_active_stake_during_heaviest_fork,
                    shred_version: SHRED_VERSION as u32,
                    wallclock: now,
                },
            );
        }

        assert!(wen_restart_thread_handle.join().is_ok());
        exit.store(true, Ordering::Relaxed);
        let progress = read_wen_restart_records(&test_state.wen_restart_proto_path).unwrap();
        let progress_start_time = progress
            .my_last_voted_fork_slots
            .as_ref()
            .unwrap()
            .wallclock;
        let mut expected_slots_stake_map: HashMap<Slot, u64> = test_state
            .last_voted_fork_slots
            .iter()
            .map(|slot| {
                (
                    *slot,
                    WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT * TOTAL_VALIDATOR_COUNT as u64,
                )
            })
            .collect();
        expected_slots_stake_map.extend(
            expected_slots_to_repair
                .iter()
                .map(|slot| (*slot, total_active_stake_during_heaviest_fork)),
        );
        assert_eq!(
            progress,
            WenRestartProgress {
                state: RestartState::Done.into(),
                my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                    last_voted_fork_slots: test_state.last_voted_fork_slots,
                    last_vote_bankhash: last_vote_bankhash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: progress_start_time,
                }),
                last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                    received: expected_received_last_voted_fork_slots,
                    final_result: Some(LastVotedForkSlotsAggregateFinal {
                        slots_stake_map: expected_slots_stake_map,
                        // We are simulating 5% joined LastVotedForkSlots but not HeaviestFork.
                        total_active_stake: total_active_stake_during_heaviest_fork + 100,
                    }),
                }),
                my_heaviest_fork: Some(HeaviestForkRecord {
                    slot: expected_heaviest_fork_slot,
                    bankhash: progress
                        .my_heaviest_fork
                        .as_ref()
                        .unwrap()
                        .bankhash
                        .to_string(),
                    total_active_stake: total_active_stake_during_heaviest_fork,
                    shred_version: SHRED_VERSION as u32,
                    wallclock: 0,
                }),
                heaviest_fork_aggregate: Some(HeaviestForkAggregateRecord {
                    received: expected_received_heaviest_fork,
                    final_result: Some(HeaviestForkAggregateFinal {
                        total_active_stake: total_active_stake_during_heaviest_fork,
                        total_active_stake_seen_supermajority:
                            total_active_stake_during_heaviest_fork,
                        total_active_stake_agreed_with_me: total_active_stake_during_heaviest_fork,
                    }),
                }),
                my_snapshot: Some(GenerateSnapshotRecord {
                    slot: expected_heaviest_fork_slot,
                    bankhash: progress.my_snapshot.as_ref().unwrap().bankhash.clone(),
                    shred_version: progress.my_snapshot.as_ref().unwrap().shred_version,
                    path: progress.my_snapshot.as_ref().unwrap().path.clone(),
                }),
            }
        );
    }

    fn change_proto_file_readonly(wen_restart_proto_path: &PathBuf, readonly: bool) {
        let mut perms = std::fs::metadata(wen_restart_proto_path)
            .unwrap()
            .permissions();
        perms.set_readonly(readonly);
        std::fs::set_permissions(wen_restart_proto_path, perms).unwrap();
    }

    #[test]
    fn test_wen_restart_initialize() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let test_state = wen_restart_test_init(&ledger_path);
        let last_vote_bankhash = Hash::new_unique();
        let mut last_voted_fork_slots = test_state.last_voted_fork_slots.clone();
        last_voted_fork_slots.reverse();
        let mut file = File::create(&test_state.wen_restart_proto_path).unwrap();
        file.write_all(b"garbage").unwrap();
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                VoteTransaction::from(Vote::new(last_voted_fork_slots.clone(), last_vote_bankhash)),
                test_state.blockstore.clone()
            )
            .unwrap_err()
            .downcast::<prost::DecodeError>()
            .unwrap(),
            prost::DecodeError::new("invalid wire type value: 7")
        );
        assert!(remove_file(&test_state.wen_restart_proto_path).is_ok());
        let last_vote_bankhash = Hash::new_unique();
        let empty_last_vote = VoteTransaction::from(Vote::new(vec![], last_vote_bankhash));
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                empty_last_vote.clone(),
                test_state.blockstore.clone()
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::MissingLastVotedForkSlots,
        );
        assert!(write_wen_restart_records(
            &test_state.wen_restart_proto_path,
            &WenRestartProgress {
                state: RestartState::LastVotedForkSlots.into(),
                ..Default::default()
            },
        )
        .is_ok());
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                VoteTransaction::from(Vote::new(last_voted_fork_slots.clone(), last_vote_bankhash)),
                test_state.blockstore.clone()
            )
            .err()
            .unwrap()
            .to_string(),
            "Malformed last voted fork slots protobuf: None"
        );
        let progress_missing_heaviest_fork_aggregate = WenRestartProgress {
            state: RestartState::HeaviestFork.into(),
            my_heaviest_fork: Some(HeaviestForkRecord {
                slot: 0,
                bankhash: Hash::new_unique().to_string(),
                total_active_stake: 0,
                shred_version: SHRED_VERSION as u32,
                wallclock: 0,
            }),
            ..Default::default()
        };
        assert!(write_wen_restart_records(
            &test_state.wen_restart_proto_path,
            &progress_missing_heaviest_fork_aggregate,
        )
        .is_ok());
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                VoteTransaction::from(Vote::new(last_voted_fork_slots.clone(), last_vote_bankhash)),
                test_state.blockstore.clone()
            ).err()
            .unwrap()
            .to_string(),
            "Malformed progress: HeaviestFork missing final_result in last_voted_fork_slots_aggregate",
        );
        let progress_missing_my_heaviestfork = WenRestartProgress {
            state: RestartState::GenerateSnapshot.into(),
            my_snapshot: Some(GenerateSnapshotRecord {
                slot: 0,
                bankhash: Hash::new_unique().to_string(),
                shred_version: SHRED_VERSION as u32,
                path: "/path/to/snapshot".to_string(),
            }),
            ..Default::default()
        };
        assert!(write_wen_restart_records(
            &test_state.wen_restart_proto_path,
            &progress_missing_my_heaviestfork,
        )
        .is_ok());
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                VoteTransaction::from(Vote::new(last_voted_fork_slots.clone(), last_vote_bankhash)),
                test_state.blockstore.clone()
            )
            .err()
            .unwrap()
            .to_string(),
            "Malformed progress: GenerateSnapshot missing my_heaviest_fork",
        );

        // Now test successful initialization.
        assert!(remove_file(&test_state.wen_restart_proto_path).is_ok());
        // Test the case where the file is not found.
        let mut vote = TowerSync::from(vec![(test_state.last_voted_fork_slots[0], 1)]);
        vote.hash = last_vote_bankhash;
        let last_vote = VoteTransaction::from(vote);
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                last_vote.clone(),
                test_state.blockstore.clone()
            )
            .unwrap(),
            (
                WenRestartProgressInternalState::Init {
                    last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                    last_vote_bankhash
                },
                WenRestartProgress {
                    state: RestartState::Init.into(),
                    ..Default::default()
                }
            )
        );
        let progress = WenRestartProgress {
            state: RestartState::Init.into(),
            my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                last_vote_bankhash: last_vote_bankhash.to_string(),
                shred_version: SHRED_VERSION as u32,
                wallclock: 0,
            }),
            ..Default::default()
        };
        assert!(write_wen_restart_records(&test_state.wen_restart_proto_path, &progress,).is_ok());
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                last_vote.clone(),
                test_state.blockstore.clone()
            )
            .unwrap(),
            (
                WenRestartProgressInternalState::Init {
                    last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                    last_vote_bankhash,
                },
                progress
            )
        );
        let progress = WenRestartProgress {
            state: RestartState::LastVotedForkSlots.into(),
            my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                last_vote_bankhash: last_vote_bankhash.to_string(),
                shred_version: SHRED_VERSION as u32,
                wallclock: 0,
            }),
            ..Default::default()
        };
        assert!(write_wen_restart_records(&test_state.wen_restart_proto_path, &progress,).is_ok());
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                last_vote.clone(),
                test_state.blockstore.clone()
            )
            .unwrap(),
            (
                WenRestartProgressInternalState::LastVotedForkSlots {
                    last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                    aggregate_final_result: None,
                },
                progress
            )
        );
        let progress = WenRestartProgress {
            state: RestartState::HeaviestFork.into(),
            my_heaviest_fork: Some(HeaviestForkRecord {
                slot: 0,
                bankhash: Hash::new_unique().to_string(),
                total_active_stake: 0,
                shred_version: SHRED_VERSION as u32,
                wallclock: 0,
            }),
            last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                received: HashMap::new(),
                final_result: Some(LastVotedForkSlotsAggregateFinal {
                    slots_stake_map: HashMap::new(),
                    total_active_stake: 1000,
                }),
            }),
            ..Default::default()
        };
        assert!(write_wen_restart_records(&test_state.wen_restart_proto_path, &progress,).is_ok());
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                last_vote.clone(),
                test_state.blockstore.clone()
            )
            .unwrap(),
            (
                WenRestartProgressInternalState::FindHeaviestFork {
                    aggregate_final_result: LastVotedForkSlotsFinalResult {
                        slots_stake_map: HashMap::new(),
                        total_active_stake: 1000,
                    },
                    my_heaviest_fork: progress.my_heaviest_fork.clone(),
                },
                progress
            )
        );
        let progress = WenRestartProgress {
            state: RestartState::GenerateSnapshot.into(),
            my_heaviest_fork: Some(HeaviestForkRecord {
                slot: 0,
                bankhash: Hash::new_unique().to_string(),
                total_active_stake: 0,
                shred_version: SHRED_VERSION as u32,
                wallclock: 0,
            }),
            my_snapshot: Some(GenerateSnapshotRecord {
                slot: 0,
                bankhash: Hash::new_unique().to_string(),
                shred_version: SHRED_VERSION as u32,
                path: "/path/to/snapshot".to_string(),
            }),
            ..Default::default()
        };
        assert!(write_wen_restart_records(&test_state.wen_restart_proto_path, &progress,).is_ok());
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                VoteTransaction::from(Vote::new(last_voted_fork_slots.clone(), last_vote_bankhash)),
                test_state.blockstore.clone()
            )
            .unwrap(),
            (
                WenRestartProgressInternalState::GenerateSnapshot {
                    new_root_slot: 0,
                    my_snapshot: progress.my_snapshot.clone(),
                },
                progress,
            )
        );
        let progress = WenRestartProgress {
            state: RestartState::Done.into(),
            ..Default::default()
        };
        assert!(write_wen_restart_records(&test_state.wen_restart_proto_path, &progress,).is_ok());
        assert_eq!(
            initialize(
                &test_state.wen_restart_proto_path,
                VoteTransaction::from(Vote::new(last_voted_fork_slots, last_vote_bankhash)),
                test_state.blockstore.clone()
            )
            .unwrap(),
            (WenRestartProgressInternalState::Done, progress)
        );
    }

    #[test]
    fn test_wen_restart_send_last_voted_fork_failures() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let test_state = wen_restart_test_init(&ledger_path);
        let progress = wen_restart_proto::WenRestartProgress {
            state: RestartState::Init.into(),
            ..Default::default()
        };
        let original_progress = progress.clone();
        assert_eq!(
            send_restart_last_voted_fork_slots(
                test_state.cluster_info.clone(),
                &[],
                Hash::new_unique(),
            )
            .err()
            .unwrap()
            .to_string(),
            "Last voted fork cannot be empty"
        );
        assert_eq!(progress, original_progress);
        let last_vote_bankhash = Hash::new_unique();
        let last_voted_fork_slots = test_state.last_voted_fork_slots.clone();
        wen_restart_test_succeed_after_failure(
            test_state,
            last_vote_bankhash,
            WenRestartProgress {
                state: RestartState::LastVotedForkSlots.into(),
                my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                    last_voted_fork_slots,
                    last_vote_bankhash: last_vote_bankhash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: 0,
                }),
                last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                    received: HashMap::new(),
                    final_result: None,
                }),
                ..Default::default()
            },
        );
    }

    #[test]
    fn test_write_wen_restart_records_failure() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let test_state = wen_restart_test_init(&ledger_path);
        let progress = wen_restart_proto::WenRestartProgress {
            state: RestartState::Init.into(),
            ..Default::default()
        };
        assert!(write_wen_restart_records(&test_state.wen_restart_proto_path, &progress).is_ok());
        change_proto_file_readonly(&test_state.wen_restart_proto_path, true);
        assert_eq!(
            write_wen_restart_records(&test_state.wen_restart_proto_path, &progress)
                .unwrap_err()
                .downcast::<std::io::Error>()
                .unwrap()
                .kind(),
            std::io::ErrorKind::PermissionDenied,
        );
        change_proto_file_readonly(&test_state.wen_restart_proto_path, false);
        assert!(write_wen_restart_records(&test_state.wen_restart_proto_path, &progress).is_ok());
        let last_voted_fork_slots = test_state.last_voted_fork_slots.clone();
        let last_vote_bankhash = Hash::new_unique();
        wen_restart_test_succeed_after_failure(
            test_state,
            last_vote_bankhash,
            WenRestartProgress {
                state: RestartState::LastVotedForkSlots.into(),
                my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                    last_voted_fork_slots,
                    last_vote_bankhash: last_vote_bankhash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: 0,
                }),
                last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                    received: HashMap::new(),
                    final_result: None,
                }),
                ..Default::default()
            },
        );
    }

    #[test]
    fn test_wen_restart_aggregate_last_voted_fork_stop_and_restart() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let test_state = wen_restart_test_init(&ledger_path);
        let last_vote_slot: Slot = test_state.last_voted_fork_slots[0];
        let last_vote_bankhash = Hash::new_unique();
        let start_time = timestamp();
        assert!(write_wen_restart_records(
            &test_state.wen_restart_proto_path,
            &WenRestartProgress {
                state: RestartState::LastVotedForkSlots.into(),
                my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                    last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                    last_vote_bankhash: last_vote_bankhash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: start_time,
                }),
                last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                    received: HashMap::new(),
                    final_result: None,
                }),
                ..Default::default()
            }
        )
        .is_ok());
        let mut rng = rand::thread_rng();
        let mut expected_messages = HashMap::new();
        let expected_slots_to_repair: Vec<Slot> =
            (last_vote_slot + 1..last_vote_slot + 3).collect();
        let mut last_voted_fork_slots_from_others = test_state.last_voted_fork_slots.clone();
        last_voted_fork_slots_from_others.reverse();
        last_voted_fork_slots_from_others.append(&mut expected_slots_to_repair.clone());
        let progress = WenRestartProgress {
            state: RestartState::LastVotedForkSlots.into(),
            my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                last_vote_bankhash: last_vote_bankhash.to_string(),
                shred_version: SHRED_VERSION as u32,
                wallclock: start_time,
            }),
            ..Default::default()
        };
        let validators_to_take: usize =
            (WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT * TOTAL_VALIDATOR_COUNT as u64 / 100 - 1)
                .try_into()
                .unwrap();
        for keypairs in test_state
            .validator_voting_keypairs
            .iter()
            .take(validators_to_take)
        {
            let wen_restart_proto_path_clone = test_state.wen_restart_proto_path.clone();
            let cluster_info_clone = test_state.cluster_info.clone();
            let bank_forks_clone = test_state.bank_forks.clone();
            let blockstore_clone = test_state.blockstore.clone();
            let exit = Arc::new(AtomicBool::new(false));
            let exit_clone = exit.clone();
            let mut progress_clone = progress.clone();
            let last_voted_fork_slots = test_state.last_voted_fork_slots.clone();
            let wen_restart_thread_handle = Builder::new()
                .name("solana-wen-restart".to_string())
                .spawn(move || {
                    assert!(aggregate_restart_last_voted_fork_slots(
                        &wen_restart_proto_path_clone,
                        WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT,
                        cluster_info_clone,
                        &last_voted_fork_slots,
                        bank_forks_clone,
                        blockstore_clone,
                        Arc::new(RwLock::new(Vec::new())),
                        exit_clone,
                        &mut progress_clone,
                    )
                    .is_ok());
                })
                .unwrap();
            let node_pubkey = keypairs.node_keypair.pubkey();
            let node = ContactInfo::new_rand(&mut rng, Some(node_pubkey));
            let last_vote_hash = Hash::new_unique();
            let now = timestamp();
            push_restart_last_voted_fork_slots(
                test_state.cluster_info.clone(),
                &node,
                &last_voted_fork_slots_from_others,
                &last_vote_hash,
                &keypairs.node_keypair,
                now,
            );
            expected_messages.insert(
                node_pubkey.to_string(),
                LastVotedForkSlotsRecord {
                    last_voted_fork_slots: last_voted_fork_slots_from_others.clone(),
                    last_vote_bankhash: last_vote_hash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: now,
                },
            );
            wait_on_expected_progress_with_timeout(
                test_state.wen_restart_proto_path.clone(),
                WenRestartProgress {
                    state: RestartState::LastVotedForkSlots.into(),
                    my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                        last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                        last_vote_bankhash: last_vote_bankhash.to_string(),
                        shred_version: SHRED_VERSION as u32,
                        wallclock: start_time,
                    }),
                    last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                        received: expected_messages.clone(),
                        final_result: None,
                    }),
                    ..Default::default()
                },
            );
            exit.store(true, Ordering::Relaxed);
            let _ = wen_restart_thread_handle.join();
        }

        // Simulating successful repair of missing blocks.
        let _ = insert_slots_into_blockstore(
            test_state.blockstore.clone(),
            last_vote_slot,
            &expected_slots_to_repair,
            TICKS_PER_SLOT,
            test_state.last_blockhash,
        );

        let last_voted_fork_slots = test_state.last_voted_fork_slots.clone();
        wen_restart_test_succeed_after_failure(
            test_state,
            last_vote_bankhash,
            WenRestartProgress {
                state: RestartState::LastVotedForkSlots.into(),
                my_last_voted_fork_slots: Some(LastVotedForkSlotsRecord {
                    last_voted_fork_slots,
                    last_vote_bankhash: last_vote_bankhash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: start_time,
                }),
                last_voted_fork_slots_aggregate: Some(LastVotedForkSlotsAggregateRecord {
                    received: expected_messages,
                    final_result: None,
                }),
                ..Default::default()
            },
        );
    }

    #[test]
    fn test_increment_and_write_wen_restart_records() {
        solana_logger::setup();
        let my_dir = TempDir::new().unwrap();
        let mut wen_restart_proto_path = my_dir.path().to_path_buf();
        wen_restart_proto_path.push("wen_restart_status.proto");
        let last_vote_bankhash = Hash::new_unique();
        let my_last_voted_fork_slots = Some(LastVotedForkSlotsRecord {
            last_voted_fork_slots: vec![0, 1],
            last_vote_bankhash: last_vote_bankhash.to_string(),
            shred_version: 0,
            wallclock: 0,
        });
        let last_voted_fork_slots_aggregate = Some(LastVotedForkSlotsAggregateRecord {
            received: HashMap::new(),
            final_result: Some(LastVotedForkSlotsAggregateFinal {
                slots_stake_map: vec![(0, 900), (1, 800)].into_iter().collect(),
                total_active_stake: 900,
            }),
        });
        let my_heaviest_fork = Some(HeaviestForkRecord {
            slot: 1,
            bankhash: Hash::default().to_string(),
            total_active_stake: 900,
            shred_version: SHRED_VERSION as u32,
            wallclock: 0,
        });
        let my_snapshot = Some(GenerateSnapshotRecord {
            slot: 1,
            bankhash: Hash::new_unique().to_string(),
            path: "snapshot_1".to_string(),
            shred_version: SHRED_VERSION as u32,
        });
        let heaviest_fork_aggregate = Some(HeaviestForkAggregateRecord {
            received: HashMap::new(),
            final_result: Some(HeaviestForkAggregateFinal {
                total_active_stake: 900,
                total_active_stake_seen_supermajority: 900,
                total_active_stake_agreed_with_me: 900,
            }),
        });
        let expected_slots_stake_map: HashMap<Slot, u64> =
            vec![(0, 900), (1, 800)].into_iter().collect();
        for (entrance_state, exit_state, entrance_progress, exit_progress) in [
            (
                WenRestartProgressInternalState::Init {
                    last_voted_fork_slots: vec![0, 1],
                    last_vote_bankhash,
                },
                WenRestartProgressInternalState::LastVotedForkSlots {
                    last_voted_fork_slots: vec![0, 1],
                    aggregate_final_result: None,
                },
                WenRestartProgress {
                    state: RestartState::LastVotedForkSlots.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    ..Default::default()
                },
                WenRestartProgress {
                    state: RestartState::LastVotedForkSlots.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    ..Default::default()
                },
            ),
            (
                WenRestartProgressInternalState::LastVotedForkSlots {
                    last_voted_fork_slots: vec![0, 1],
                    aggregate_final_result: Some(LastVotedForkSlotsFinalResult {
                        slots_stake_map: expected_slots_stake_map.clone(),
                        total_active_stake: 900,
                    }),
                },
                WenRestartProgressInternalState::FindHeaviestFork {
                    aggregate_final_result: LastVotedForkSlotsFinalResult {
                        slots_stake_map: expected_slots_stake_map.clone(),
                        total_active_stake: 900,
                    },
                    my_heaviest_fork: None,
                },
                WenRestartProgress {
                    state: RestartState::LastVotedForkSlots.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
                    ..Default::default()
                },
                WenRestartProgress {
                    state: RestartState::HeaviestFork.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
                    ..Default::default()
                },
            ),
            (
                WenRestartProgressInternalState::FindHeaviestFork {
                    aggregate_final_result: LastVotedForkSlotsFinalResult {
                        slots_stake_map: expected_slots_stake_map,
                        total_active_stake: 900,
                    },
                    my_heaviest_fork: Some(HeaviestForkRecord {
                        slot: 1,
                        bankhash: Hash::default().to_string(),
                        total_active_stake: 900,
                        shred_version: SHRED_VERSION as u32,
                        wallclock: 0,
                    }),
                },
                WenRestartProgressInternalState::HeaviestFork { new_root_slot: 1 },
                WenRestartProgress {
                    state: RestartState::HeaviestFork.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
                    ..Default::default()
                },
                WenRestartProgress {
                    state: RestartState::HeaviestFork.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
                    my_heaviest_fork: my_heaviest_fork.clone(),
                    ..Default::default()
                },
            ),
            (
                WenRestartProgressInternalState::HeaviestFork { new_root_slot: 1 },
                WenRestartProgressInternalState::GenerateSnapshot {
                    new_root_slot: 1,
                    my_snapshot: None,
                },
                WenRestartProgress {
                    state: RestartState::HeaviestFork.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
                    my_heaviest_fork: my_heaviest_fork.clone(),
                    heaviest_fork_aggregate: heaviest_fork_aggregate.clone(),
                    ..Default::default()
                },
                WenRestartProgress {
                    state: RestartState::GenerateSnapshot.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
                    my_heaviest_fork: my_heaviest_fork.clone(),
                    heaviest_fork_aggregate: heaviest_fork_aggregate.clone(),
                    ..Default::default()
                },
            ),
            (
                WenRestartProgressInternalState::GenerateSnapshot {
                    new_root_slot: 1,
                    my_snapshot: my_snapshot.clone(),
                },
                WenRestartProgressInternalState::Done,
                WenRestartProgress {
                    state: RestartState::HeaviestFork.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
                    my_heaviest_fork: my_heaviest_fork.clone(),
                    heaviest_fork_aggregate: heaviest_fork_aggregate.clone(),
                    ..Default::default()
                },
                WenRestartProgress {
                    state: RestartState::Done.into(),
                    my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
                    last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
                    my_heaviest_fork: my_heaviest_fork.clone(),
                    heaviest_fork_aggregate,
                    my_snapshot: my_snapshot.clone(),
                },
            ),
        ] {
            let mut progress = entrance_progress;
            let state = increment_and_write_wen_restart_records(
                &wen_restart_proto_path,
                entrance_state,
                &mut progress,
            )
            .unwrap();
            assert_eq!(&state, &exit_state);
            assert_eq!(&progress, &exit_progress);
        }
        let mut progress = WenRestartProgress {
            state: RestartState::Done.into(),
            my_last_voted_fork_slots: my_last_voted_fork_slots.clone(),
            last_voted_fork_slots_aggregate: last_voted_fork_slots_aggregate.clone(),
            ..Default::default()
        };
        assert_eq!(
            increment_and_write_wen_restart_records(
                &wen_restart_proto_path,
                WenRestartProgressInternalState::Done,
                &mut progress
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::UnexpectedState(RestartState::Done),
        );
    }

    #[test]
    fn test_find_heaviest_fork_failures() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let exit = Arc::new(AtomicBool::new(false));
        let test_state = wen_restart_test_init(&ledger_path);
        let last_vote_slot = test_state.last_voted_fork_slots[0];
        let slot_with_no_block = last_vote_slot + 5;
        // This fails because corresponding block is not found, which is wrong, we should have
        // repaired all eligible blocks when we exit LastVotedForkSlots state.
        assert_eq!(
            find_heaviest_fork(
                LastVotedForkSlotsFinalResult {
                    slots_stake_map: vec![(0, 900), (slot_with_no_block, 800)]
                        .into_iter()
                        .collect(),
                    total_active_stake: 900,
                },
                test_state.bank_forks.clone(),
                test_state.blockstore.clone(),
                exit.clone(),
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::BlockNotFound(slot_with_no_block),
        );
        // The following fails because we expect to see the first slot in slots_stake_map doesn't chain to local root.
        assert_eq!(
            find_heaviest_fork(
                LastVotedForkSlotsFinalResult {
                    slots_stake_map: vec![(last_vote_slot, 900)].into_iter().collect(),
                    total_active_stake: 900,
                },
                test_state.bank_forks.clone(),
                test_state.blockstore.clone(),
                exit.clone(),
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::BlockNotLinkedToExpectedParent(
                last_vote_slot,
                Some(last_vote_slot - 1),
                0
            ),
        );
        // The following fails because we expect to see the some slot in slots_stake_map doesn't chain to the
        // one before it.
        assert_eq!(
            find_heaviest_fork(
                LastVotedForkSlotsFinalResult {
                    slots_stake_map: vec![(2, 900), (last_vote_slot, 900)].into_iter().collect(),
                    total_active_stake: 900,
                },
                test_state.bank_forks.clone(),
                test_state.blockstore.clone(),
                exit.clone(),
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::BlockNotLinkedToExpectedParent(
                last_vote_slot,
                Some(last_vote_slot - 1),
                2
            ),
        );
        // The following fails because the new slot is not full.
        let not_full_slot = last_vote_slot + 5;
        let parent_slot = last_vote_slot;
        let num_slots = (not_full_slot - parent_slot).max(1);
        let mut entries = create_ticks(num_slots * TICKS_PER_SLOT, 0, test_state.last_blockhash);
        assert!(entries.len() > 1);
        entries.pop();
        let shreds = entries_to_test_shreds(
            &entries,
            not_full_slot,
            parent_slot,
            false,
            0,
            true, // merkle_variant
        );
        test_state
            .blockstore
            .insert_shreds(shreds, None, false)
            .unwrap();
        let mut slots_stake_map: HashMap<Slot, u64> = test_state
            .last_voted_fork_slots
            .iter()
            .map(|slot| (*slot, 900))
            .collect();
        slots_stake_map.insert(not_full_slot, 800);
        assert_eq!(
            find_heaviest_fork(
                LastVotedForkSlotsFinalResult {
                    slots_stake_map,
                    total_active_stake: 900,
                },
                test_state.bank_forks.clone(),
                test_state.blockstore.clone(),
                exit.clone(),
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::BlockNotFull(not_full_slot)
        );
        // The following fails because we added two blocks at the end of the chain, they are full in blockstore
        // but the parent of the first one is missing.
        let missing_parent = last_vote_slot.saturating_add(1);
        let new_slot = last_vote_slot.saturating_add(2);
        let new_hash = insert_slots_into_blockstore(
            test_state.blockstore.clone(),
            last_vote_slot,
            &[missing_parent],
            1,
            test_state.last_blockhash,
        );
        let _ = insert_slots_into_blockstore(
            test_state.blockstore.clone(),
            missing_parent,
            &[new_slot],
            TICKS_PER_SLOT,
            new_hash,
        );
        let mut slots_stake_map: HashMap<Slot, u64> = test_state
            .last_voted_fork_slots
            .iter()
            .map(|slot| (*slot, 900))
            .collect();
        slots_stake_map.insert(missing_parent, 800);
        slots_stake_map.insert(new_slot, 800);
        assert_eq!(
            find_heaviest_fork(
                LastVotedForkSlotsFinalResult {
                    slots_stake_map,
                    total_active_stake: 900,
                },
                test_state.bank_forks.clone(),
                test_state.blockstore.clone(),
                exit.clone(),
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::BlockNotFrozenAfterReplay(
                missing_parent,
                Some("invalid block error: incomplete block".to_string())
            ),
        );
    }

    fn start_aggregate_heaviest_fork_thread(
        test_state: &WenRestartTestInitResult,
        heaviest_fork_slot: Slot,
        heaviest_fork_bankhash: Hash,
        exit: Arc<AtomicBool>,
        expected_error: Option<WenRestartError>,
    ) -> std::thread::JoinHandle<()> {
        let progress = wen_restart_proto::WenRestartProgress {
            state: RestartState::HeaviestFork.into(),
            my_heaviest_fork: Some(HeaviestForkRecord {
                slot: heaviest_fork_slot,
                bankhash: heaviest_fork_bankhash.to_string(),
                total_active_stake: WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT
                    .saturating_mul(TOTAL_VALIDATOR_COUNT as u64),
                shred_version: SHRED_VERSION as u32,
                wallclock: 0,
            }),
            ..Default::default()
        };
        let wen_restart_path = test_state.wen_restart_proto_path.clone();
        let cluster_info = test_state.cluster_info.clone();
        let bank_forks = test_state.bank_forks.clone();
        Builder::new()
            .name("solana-wen-restart-aggregate-heaviest-fork".to_string())
            .spawn(move || {
                let result = aggregate_restart_heaviest_fork(
                    &wen_restart_path,
                    WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT,
                    cluster_info,
                    bank_forks,
                    exit,
                    &mut progress.clone(),
                );
                if let Some(expected_error) = expected_error {
                    assert_eq!(
                        result.unwrap_err().downcast::<WenRestartError>().unwrap(),
                        expected_error
                    );
                } else {
                    assert!(result.is_ok());
                }
            })
            .unwrap()
    }

    #[test]
    fn test_aggregate_heaviest_fork_send_gossip_early() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let test_state = wen_restart_test_init(&ledger_path);
        let heaviest_fork_slot = test_state.last_voted_fork_slots[0] + 3;
        let heaviest_fork_bankhash = Hash::new_unique();

        let mut cursor = solana_gossip::crds::Cursor::default();
        // clear the heaviest fork queue so we make sure a new HeaviestFork is sent out later.
        let _ = test_state
            .cluster_info
            .get_restart_heaviest_fork(&mut cursor);

        let exit = Arc::new(AtomicBool::new(false));
        let thread = start_aggregate_heaviest_fork_thread(
            &test_state,
            heaviest_fork_slot,
            heaviest_fork_bankhash,
            exit.clone(),
            Some(WenRestartError::Exiting),
        );
        // Simulating everyone sending out the first RestartHeaviestFork message, Gossip propagation takes
        // time, so the observed_stake is probably smaller than actual active stake. We should send out
        // heaviest fork indicating we have active stake exceeding supermajority.
        let validators_to_take: usize = ((WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT
            - NON_CONFORMING_VALIDATOR_PERCENT)
            * TOTAL_VALIDATOR_COUNT as u64
            / 100
            - 1)
        .try_into()
        .unwrap();
        for keypair in test_state
            .validator_voting_keypairs
            .iter()
            .take(validators_to_take)
        {
            let node_pubkey = keypair.node_keypair.pubkey();
            let node = ContactInfo::new_rand(&mut rand::thread_rng(), Some(node_pubkey));
            let now = timestamp();
            push_restart_heaviest_fork(
                test_state.cluster_info.clone(),
                &node,
                heaviest_fork_slot,
                &heaviest_fork_bankhash,
                100,
                &keypair.node_keypair,
                now,
            );
        }
        let my_pubkey = test_state.cluster_info.id();
        let mut found_myself = false;
        let expected_active_stake = (WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT
            - NON_CONFORMING_VALIDATOR_PERCENT)
            * TOTAL_VALIDATOR_COUNT as u64;
        while !found_myself {
            sleep(Duration::from_millis(100));
            test_state.cluster_info.flush_push_queue();
            for gossip_record in test_state
                .cluster_info
                .get_restart_heaviest_fork(&mut cursor)
            {
                if gossip_record.from == my_pubkey
                    && gossip_record.observed_stake == expected_active_stake
                {
                    found_myself = true;
                    break;
                }
            }
        }
        exit.store(true, Ordering::Relaxed);
        assert!(thread.join().is_ok());
    }

    #[test]
    fn test_aggregate_heaviest_fork() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let test_state = wen_restart_test_init(&ledger_path);
        let heaviest_fork_slot = test_state.last_voted_fork_slots[0] + 3;
        let heaviest_fork_bankhash = Hash::new_unique();
        let expected_active_stake = (WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT
            - NON_CONFORMING_VALIDATOR_PERCENT)
            * TOTAL_VALIDATOR_COUNT as u64;
        let progress = wen_restart_proto::WenRestartProgress {
            state: RestartState::HeaviestFork.into(),
            my_heaviest_fork: Some(HeaviestForkRecord {
                slot: heaviest_fork_slot,
                bankhash: heaviest_fork_bankhash.to_string(),
                total_active_stake: expected_active_stake,
                shred_version: SHRED_VERSION as u32,
                wallclock: 0,
            }),
            ..Default::default()
        };
        let different_bankhash = Hash::new_unique();
        let validators_to_take: usize = ((WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT
            - NON_CONFORMING_VALIDATOR_PERCENT)
            * TOTAL_VALIDATOR_COUNT as u64
            / 100
            - 1)
        .try_into()
        .unwrap();
        for keypair in test_state
            .validator_voting_keypairs
            .iter()
            .take(validators_to_take)
        {
            let node_pubkey = keypair.node_keypair.pubkey();
            let node = ContactInfo::new_rand(&mut rand::thread_rng(), Some(node_pubkey));
            let now = timestamp();
            push_restart_heaviest_fork(
                test_state.cluster_info.clone(),
                &node,
                heaviest_fork_slot,
                &different_bankhash,
                expected_active_stake,
                &keypair.node_keypair,
                now,
            );
        }
        let mut expected_block_stake_map = HashMap::new();
        expected_block_stake_map.insert((heaviest_fork_slot, heaviest_fork_bankhash), 100);
        expected_block_stake_map.insert((heaviest_fork_slot, different_bankhash), 1400);
        assert_eq!(
            aggregate_restart_heaviest_fork(
                &test_state.wen_restart_proto_path,
                WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT,
                test_state.cluster_info.clone(),
                test_state.bank_forks.clone(),
                Arc::new(AtomicBool::new(false)),
                &mut progress.clone(),
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::NotEnoughStakeAgreeingWithUs(
                heaviest_fork_slot,
                heaviest_fork_bankhash,
                expected_block_stake_map
            ),
        );
        // If we have enough stake agreeing with us, we should be able to aggregate the heaviest fork.
        let validators_to_take: usize =
            (WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT * TOTAL_VALIDATOR_COUNT as u64 / 100 - 1)
                .try_into()
                .unwrap();
        for keypair in test_state
            .validator_voting_keypairs
            .iter()
            .take(validators_to_take)
        {
            let node_pubkey = keypair.node_keypair.pubkey();
            let node = ContactInfo::new_rand(&mut rand::thread_rng(), Some(node_pubkey));
            let now = timestamp();
            push_restart_heaviest_fork(
                test_state.cluster_info.clone(),
                &node,
                heaviest_fork_slot,
                &heaviest_fork_bankhash,
                expected_active_stake,
                &keypair.node_keypair,
                now,
            );
        }
        assert!(aggregate_restart_heaviest_fork(
            &test_state.wen_restart_proto_path,
            WAIT_FOR_SUPERMAJORITY_THRESHOLD_PERCENT,
            test_state.cluster_info.clone(),
            test_state.bank_forks.clone(),
            Arc::new(AtomicBool::new(false)),
            &mut progress.clone(),
        )
        .is_ok());
    }

    #[test]
    fn test_generate_snapshot() {
        solana_logger::setup();
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let test_state = wen_restart_test_init(&ledger_path);
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let snapshot_config = SnapshotConfig {
            bank_snapshots_dir: bank_snapshots_dir.as_ref().to_path_buf(),
            full_snapshot_archives_dir: full_snapshot_archives_dir.as_ref().to_path_buf(),
            incremental_snapshot_archives_dir: incremental_snapshot_archives_dir
                .as_ref()
                .to_path_buf(),
            ..Default::default()
        };
        let old_root_bank = test_state.bank_forks.read().unwrap().root_bank();
        let old_root_slot = old_root_bank.slot();
        let last_vote_slot = test_state.last_voted_fork_slots[0];
        let exit = Arc::new(AtomicBool::new(false));
        let mut slots = test_state.last_voted_fork_slots.clone();
        slots.reverse();
        let old_last_vote_bankhash = find_bankhash_of_heaviest_fork(
            last_vote_slot,
            slots,
            test_state.blockstore.clone(),
            test_state.bank_forks.clone(),
            old_root_bank,
            &exit,
        )
        .unwrap();
        test_state
            .bank_forks
            .write()
            .unwrap()
            .set_snapshot_config(Some(snapshot_config.clone()));
        let old_root_bank = test_state.bank_forks.read().unwrap().root_bank();
        // Trigger full snapshot generation on the old root bank.
        assert!(bank_to_full_snapshot_archive(
            snapshot_config.bank_snapshots_dir.clone(),
            &old_root_bank,
            Some(snapshot_config.snapshot_version),
            snapshot_config.full_snapshot_archives_dir.clone(),
            snapshot_config.incremental_snapshot_archives_dir.clone(),
            snapshot_config.archive_format,
        )
        .is_ok());
        let generated_record = generate_snapshot(
            test_state.bank_forks.clone(),
            &snapshot_config,
            &AbsRequestSender::default(),
            test_state.genesis_config_hash,
            last_vote_slot,
        )
        .unwrap();
        let new_root_bankhash = test_state
            .bank_forks
            .read()
            .unwrap()
            .get(last_vote_slot)
            .unwrap()
            .hash();
        assert_ne!(old_last_vote_bankhash, new_root_bankhash);
        let new_shred_version = generated_record.shred_version;
        assert_ne!(new_shred_version, SHRED_VERSION as u32);
        let snapshot_hash = Hash::from_str(
            generated_record
                .path
                .split('-')
                .last()
                .unwrap()
                .split('.')
                .next()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            generated_record,
            GenerateSnapshotRecord {
                slot: last_vote_slot,
                bankhash: new_root_bankhash.to_string(),
                shred_version: new_shred_version,
                path: build_incremental_snapshot_archive_path(
                    &snapshot_config.incremental_snapshot_archives_dir,
                    old_root_slot,
                    last_vote_slot,
                    &SnapshotHash(snapshot_hash),
                    snapshot_config.archive_format,
                )
                .display()
                .to_string(),
            },
        );
        // Now generate a snapshot for older slot, it should fail because we already
        // have a full snapshot.
        assert_eq!(
            generate_snapshot(
                test_state.bank_forks.clone(),
                &snapshot_config,
                &AbsRequestSender::default(),
                test_state.genesis_config_hash,
                old_root_slot,
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::GenerateSnapshotWhenOneExists(
                old_root_slot,
                snapshot_config
                    .full_snapshot_archives_dir
                    .to_string_lossy()
                    .to_string()
            ),
        );
        // fails if we already have an incremental snapshot (we just generated one at last_vote_slot).
        let older_slot = last_vote_slot - 1;
        assert_eq!(
            generate_snapshot(
                test_state.bank_forks.clone(),
                &snapshot_config,
                &AbsRequestSender::default(),
                test_state.genesis_config_hash,
                older_slot,
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::FutureSnapshotExists(
                older_slot,
                last_vote_slot,
                snapshot_config
                    .incremental_snapshot_archives_dir
                    .to_string_lossy()
                    .to_string()
            ),
        );
        // Generate snapshot for a slot without any block, it should fail.
        let empty_slot = last_vote_slot + 1;
        assert_eq!(
            generate_snapshot(
                test_state.bank_forks.clone(),
                &snapshot_config,
                &AbsRequestSender::default(),
                test_state.genesis_config_hash,
                empty_slot,
            )
            .unwrap_err()
            .downcast::<WenRestartError>()
            .unwrap(),
            WenRestartError::BlockNotFound(empty_slot),
        );
    }
}

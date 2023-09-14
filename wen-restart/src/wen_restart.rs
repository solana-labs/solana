//! The `wen-restart` module handles automatically repair in cluster restart

use solana_runtime::{accounts_background_service::AbsRequestSender, snapshot_config::SnapshotConfig};
use {
    crate::{
        epoch_stakes_map::EpochStakesMap,
        heaviest_fork_aggregate::HeaviestForkAggregate,
        last_voted_fork_slots_aggregate::{LastVotedForkSlotsAggregate, SlotsToRepairList},
        solana::wen_restart_proto,
    },
    log::*,
    prost::Message,
    solana_gossip::{cluster_info::ClusterInfo, crds::Cursor},
    solana_ledger::blockstore::Blockstore,
    solana_runtime::{
        bank_forks::BankForks,
        snapshot_bank_utils::bank_to_incremental_snapshot_archive,
        snapshot_utils::{
            get_highest_full_snapshot_archive_slot,
            get_incremental_snapshot_archives,
        },
    },
    solana_sdk::{clock::Slot, hash::Hash, shred_version::compute_shred_version},
    solana_vote_program::vote_state::VoteTransaction,
    std::{
        collections::{HashMap, HashSet},
        fs::{read, File},
        io::{Error, Write},
        path::PathBuf,
        str::FromStr,
        sync::{Arc, RwLock, RwLockReadGuard},
        thread::sleep,
        time::Duration,
    },
};

// The number of ancestor slots sent is hard coded at 81000, because that's
// 400ms * 81000 = 9 hours, we assume most restart decisions to be made in 9
// hours.
const LISTEN_INTERVAL_MS: u64 = 100;
const MAX_SLOTS_ON_VOTED_FORKS: u32 = 81000;
const OPTIMISTIC_CONFIRMED_THRESHOLD: f64 = 0.8;

fn select_heaviest_fork(
    new_slots: SlotsToRepairList,
    my_bank_forks: RwLockReadGuard<'_, BankForks>,
    not_active_percentage: f64,
) -> Option<(Slot, Hash, f64)> {
    let new_slots_map: HashMap<Slot, f64> = new_slots.into_iter().collect();
    let descendants = my_bank_forks.descendants();
    let mut current_bank = my_bank_forks.root_bank().clone();
    loop {
        let current_slot = current_bank.slot();
        match descendants.get(&current_slot) {
            None => break,
            Some(my_descendants) => {
                let mut selected_slot = 0;
                let mut selected_weight: f64 = 0.0;
                let mut selected_bank = current_bank.clone();
                my_descendants.iter().for_each(|slot| {
                    if let Some(my_bank) = my_bank_forks.get(*slot) {
                        if let Some(parent) = my_bank.parent() {
                            if parent.slot() == current_slot {
                                let weight = new_slots_map.get(slot).unwrap_or(&0.0);
                                if weight > &selected_weight {
                                    selected_weight = *weight;
                                    selected_slot = slot.clone();
                                    selected_bank = my_bank.clone();
                                }
                            }
                        }
                    }
                });
                if selected_weight + not_active_percentage > 0.62 {
                    current_bank = selected_bank;
                } else {
                    break;
                }
            },
        }
    }
    let slot = current_bank.slot();
    if let Some(percent) = new_slots_map.get(&slot) {
        return Some((slot, current_bank.hash(), *percent));
    } else {
        return None;
    }
}

pub fn wen_restart(
    wen_restart_path: &Option<PathBuf>,
    snapshot_config: &SnapshotConfig,
    accounts_background_request_sender: &AbsRequestSender,
    genesis_config_hash: &Hash,
    last_vote: VoteTransaction,
    blockstore: Arc<Blockstore>,
    cluster_info: Arc<ClusterInfo>,
    bank_forks: Arc<RwLock<BankForks>>,
    slots_to_repair_for_wen_restart: Arc<RwLock<Option<Vec<Slot>>>>,
) -> Result<Slot, Box<dyn std::error::Error>> {
    // repair and restart option does not work without last voted slot.
    let last_voted_slot = last_vote.last_voted_slot().unwrap();
    let mut last_voted_fork = vec![last_voted_slot];
    let mut slot = last_voted_slot;
    for _ in 0..MAX_SLOTS_ON_VOTED_FORKS {
        match blockstore.meta(slot) {
            Ok(Some(slot_meta)) => {
                match slot_meta.parent_slot {
                    Some(parent_slot) => {
                        last_voted_fork.push(parent_slot);
                        slot = parent_slot;
                    }
                    None => break,
                };
            }
            _ => break,
        }
    }
    info!(
        "wen_restart last voted fork {} {:?}",
        last_voted_slot,
        last_voted_fork
    );
    last_voted_fork.sort();
    cluster_info.push_last_voted_fork_slots(&last_voted_fork, last_vote.hash());
    let root_bank = bank_forks.read().unwrap().root_bank();
    let root_slot = root_bank.slot();
    let mut cursor = Cursor::default();
    let mut epoch_stakes_map = EpochStakesMap::new(root_bank.clone());
    let my_pubkey = cluster_info.id();
    let mut last_voted_fork_slots_aggregate =
        LastVotedForkSlotsAggregate::new(root_bank.slot(), &last_voted_fork, &my_pubkey);
    let mut is_full_slots = HashSet::new();
    // Aggregate LastVotedForkSlots until seeing this message from 80% of the validators.
    info!("wen_restart aggregating RestartLastVotedForkSlots");
    loop {
        let last_voted_fork_slots = cluster_info.get_restart_last_voted_fork_slots(&mut cursor);
        let (slots_to_repair, not_active_percentage, active_peers) =
            last_voted_fork_slots_aggregate.aggregate(last_voted_fork_slots, &mut epoch_stakes_map);
        if let Some(new_slots) = slots_to_repair {
            let my_bank_forks = bank_forks.read().unwrap();
            if epoch_stakes_map.has_missing_epochs() {
                epoch_stakes_map.update_bank(my_bank_forks.highest_frozen_bank());
            }
            let filtered_slots: Vec<Slot> = new_slots
                .into_iter()
                .map(|(slot, _)| slot)
                .filter(|slot| {
                    if slot <= &root_slot || is_full_slots.contains(slot) {
                        return false;
                    }
                    let is_full = my_bank_forks.get(*slot).map_or(false, |bank| bank.is_frozen());
                    if is_full {
                        is_full_slots.insert(slot.clone());
                    }
                    !is_full
                })
                .collect();
            *slots_to_repair_for_wen_restart.write().unwrap() = Some(filtered_slots);
        }
        info!(
            "wen_restart aggregating RestartLastVotedForkSlots currently {} peers total stake percentage {}",
            active_peers,
            1.0 - not_active_percentage
        );
        if not_active_percentage < 1.0 - OPTIMISTIC_CONFIRMED_THRESHOLD {
            break;
        }
        sleep(Duration::from_millis(LISTEN_INTERVAL_MS));
    }
    let my_selected_slot;
    let my_selected_hash;
    // Now wait until all necessary blocks are frozen.
    loop {
        let my_bank_forks = bank_forks.read().unwrap();
        if epoch_stakes_map.has_missing_epochs() {
            epoch_stakes_map.update_bank(my_bank_forks.highest_frozen_bank());
        }
        let (slots_to_repair, not_active_percentage, _) =
            last_voted_fork_slots_aggregate.aggregate(Vec::default(), &mut epoch_stakes_map);
        let new_slots = slots_to_repair.unwrap();
        let filtered_slots: Vec<Slot> = new_slots
        .iter()
        .map(|(slot, _)| slot.clone())
        .filter(|slot| {
            if slot <= &root_slot || is_full_slots.contains(slot) {
                return false;
            }
            let is_full = my_bank_forks.get(*slot).map_or(false, |bank| bank.is_frozen());
            if is_full {
                is_full_slots.insert(slot.clone());
            }
            !is_full
        })
        .collect();
        info!("wen_restart waiting for all slots frozen, slots to repair {:?}", &filtered_slots);
        let all_slots_frozen = filtered_slots.is_empty();
        *slots_to_repair_for_wen_restart.write().unwrap() = Some(filtered_slots);
        if all_slots_frozen {
            info!("wen_restart all slots frozen");
            if let Some((slot, hash, percent)) =
                select_heaviest_fork(new_slots, my_bank_forks, not_active_percentage)
            {
                info!("wen_restart selected HeaviestFork {} {} {}", slot, hash, percent);
                cluster_info.push_heaviest_fork(slot, hash, percent);
                my_selected_slot = slot;
                my_selected_hash = hash;
                break;
            }
        }
        sleep(Duration::from_millis(LISTEN_INTERVAL_MS));
    }
    // Aggregate heaviest fork and sanity check.
    let mut heaviest_fork_aggregate =
        HeaviestForkAggregate::new(my_pubkey, my_selected_slot, my_selected_hash);
    cursor = Cursor::default();
    info!("wen_restart aggregating HeaviestFork");
    loop {
        let heaviest_fork_list = cluster_info.get_heaviest_fork(&mut cursor);
        let (current_percentage, active_peers) = heaviest_fork_aggregate.aggregate(heaviest_fork_list, &mut epoch_stakes_map);
        info!("wen_restart aggregating HeaviestFork {} peers total percentage {}", active_peers, current_percentage);
        if current_percentage > 0.8 {
            info!(
                "Success, agreed on {} {}",
                my_selected_slot, my_selected_hash
            );
            break;
        }
        sleep(Duration::from_millis(LISTEN_INTERVAL_MS));
    }
    info!("wen_restart add hard forks, set_root and generate snapshot {}", my_selected_slot);
    let new_root_bank_hash;
    let new_shred_version;
    {
        let mut my_bank_forks = bank_forks.write().unwrap();
        let root_bank = my_bank_forks.root_bank();
        root_bank.register_hard_fork(my_selected_slot);
        let new_root_bank = my_bank_forks.get(my_selected_slot).unwrap();
        new_root_bank.rehash();
        new_root_bank_hash = new_root_bank.hash();
        new_root_bank.rc.accounts.accounts_db.add_root(my_selected_slot);
        bank_to_incremental_snapshot_archive(
            snapshot_config.bank_snapshots_dir.clone(),
            &new_root_bank,
            get_highest_full_snapshot_archive_slot(snapshot_config.full_snapshot_archives_dir.clone()).unwrap(),
            Some(snapshot_config.snapshot_version),
            snapshot_config.full_snapshot_archives_dir.clone(),
            snapshot_config.incremental_snapshot_archives_dir.clone(),
            snapshot_config.archive_format,
            snapshot_config.maximum_full_snapshot_archives_to_retain,
            snapshot_config.maximum_incremental_snapshot_archives_to_retain,
        )?;
        info!("wen_restart: new hash for slot {} after adding hard fork {}", my_selected_slot, &new_root_bank_hash);
        new_shred_version = compute_shred_version(
            genesis_config_hash,
            Some(&new_root_bank.hard_forks()),
        );
    }
    info!("wen_restart waiting for new snapshot to be generated on {}", my_selected_slot);
    loop {
        if get_incremental_snapshot_archives(snapshot_config.incremental_snapshot_archives_dir.clone())
            .iter()
            .any(|archive_info| archive_info.slot() == my_selected_slot)
        {
            info!("wen_restart snapshot generated {}", my_selected_slot);
            break;
        }
        sleep(Duration::from_millis(1000));
    }
    write_wen_restart_records(wen_restart_path, wen_restart_proto::WenRestartProgress {
        state: wen_restart_proto::wen_restart_progress::State::FinishedSnapshot.into(),
        snapshot_bank: Some(wen_restart_proto::wen_restart_progress::SnapshotBank{
            slot: my_selected_slot,
            bankhash: new_root_bank_hash.to_string(),
            shred_version: new_shred_version as u32,
        }),
    })?;
    Ok(my_selected_slot)
}

fn read_wen_restart_records(records_path: &Option<PathBuf>) -> Result<wen_restart_proto::WenRestartProgress, Error> {
    let buffer = read(records_path.clone().unwrap())?;
    let progress = wen_restart_proto::WenRestartProgress::decode(&mut std::io::Cursor::new(buffer))?;
    info!("Read record {:?}", progress);
    Ok(progress)
}

pub fn get_wen_restart_phase_one_result(records_path: &Option<PathBuf>) -> Result<Option<(Slot, Hash, u16)>, String> {
    match read_wen_restart_records(records_path) {
        Ok(progress) => {
            match progress.snapshot_bank {
                Some(bank) => {
                    let bankhash = Hash::from_str(&bank.bankhash).map_err(|e| format!("{e:?}"))?;
                    Ok(Some((bank.slot, bankhash, bank.shred_version.try_into().unwrap())))
                },
                None => Ok(None),
            }
        },
        Err(e) => {
            info!("wen_restart_record not found: {e:?}");
            Ok(None)
        },
    }
}

pub fn finish_wen_restart(records_path: &Option<PathBuf>) -> Result<(), String> {
    match write_wen_restart_records(records_path, wen_restart_proto::WenRestartProgress {
        state: wen_restart_proto::wen_restart_progress::State::Done.into(),
        snapshot_bank: None,
    }) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("wait_for_wen_restart failed: {e:?}")),
    }
}

fn write_wen_restart_records(records_path: &Option<PathBuf>, new_progress: wen_restart_proto::WenRestartProgress) -> Result<(), Error> {
    // overwrite anything if exists
    let mut file = File::create(records_path.clone().unwrap())?;
    info!("writing {:?}", new_progress);
    let mut buf = Vec::new();
    buf.reserve(new_progress.encoded_len());
    new_progress.encode(&mut buf)?;
    file.write(&buf)?;
    Ok(())
}
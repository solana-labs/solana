//! The `wen-restart` module handles automatically repair in cluster restart
use {
    crate::{
        epoch_stakes_map::EpochStakesMap,
        heaviest_fork_aggregate::HeaviestForkAggregate,
        last_voted_fork_slots_aggregate::{LastVotedForkSlotsAggregate, SlotsToRepairList},
    },
    crossbeam_channel::{Receiver as CrossbeamReceiver, Sender as CrossbeamSender},
    log::*,
    solana_gossip::{cluster_info::ClusterInfo, crds::Cursor},
    solana_ledger::blockstore::Blockstore,
    solana_runtime::{
        accounts_background_service::AbsRequestSender, bank_forks::BankForks,
        snapshot_archive_info::SnapshotArchiveInfoGetter, snapshot_config::SnapshotConfig,
        snapshot_utils::get_incremental_snapshot_archives,
    },
    solana_sdk::{clock::Slot, hash::Hash},
    solana_vote_program::vote_state::VoteTransaction,
    std::{
        collections::HashMap,
        sync::{Arc, RwLock, RwLockReadGuard},
        thread::sleep,
        time::Duration,
    },
};

pub type RestartSlotsToRepairSender = CrossbeamSender<Option<Vec<Slot>>>;
pub type RestartSlotsToRepairReceiver = CrossbeamReceiver<Option<Vec<Slot>>>;

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
    let mut selected_bank = my_bank_forks.root_bank();
    loop {
        let current_slot = selected_bank.slot();
        match descendants.get(&current_slot) {
            None => break,
            Some(children) => {
                let mut selected_slot = 0;
                let mut selected_weight: f64 = 0.0;
                children.iter().for_each(|slot| {
                    let weight = new_slots_map.get(slot).unwrap_or(&0.0);
                    if weight > &selected_weight {
                        selected_weight = *weight;
                        selected_slot = slot.clone();
                    }
                });
                if selected_weight + not_active_percentage > 0.62 {
                    selected_bank = my_bank_forks.get(selected_slot).unwrap();
                } else {
                    break;
                }
            }
        }
    }
    let slot = selected_bank.slot();
    if let Some(percent) = new_slots_map.get(&slot) {
        return Some((slot, selected_bank.hash(), *percent));
    } else {
        return None;
    }
}

pub fn wen_restart(
    last_vote: VoteTransaction,
    blockstore: Arc<Blockstore>,
    cluster_info: Arc<ClusterInfo>,
    bank_forks: Arc<RwLock<BankForks>>,
    restart_slots_to_repair_sender: RestartSlotsToRepairSender,
    accounts_background_request_sender: AbsRequestSender,
    snapshot_config: &SnapshotConfig,
) -> Result<Slot, Box<dyn std::error::Error>> {
    // repair and restart option does not work without last voted slot.
    let last_voted_slot = last_vote.last_voted_slot().unwrap();
    let mut last_voted_fork = vec![last_voted_slot];
    let mut slot = last_voted_slot;
    for _ in 0..MAX_SLOTS_ON_VOTED_FORKS {
        match blockstore.meta(slot) {
            Ok(Some(slot_meta)) => {
                let parent_slot = slot_meta.parent_slot.unwrap();
                last_voted_fork.push(parent_slot);
                slot = parent_slot;
            }
            _ => break,
        }
    }
    info!(
        "Starting repair and restart protocol last vote {} {:?}",
        last_voted_slot,
        last_voted_fork.len()
    );
    cluster_info.push_last_voted_fork_slots(&last_voted_fork, last_vote.hash());
    let root_bank = bank_forks.read().unwrap().root_bank();
    let mut cursor = Cursor::default();
    let mut epoch_stakes_map = EpochStakesMap::new(root_bank.clone());
    let mut last_voted_fork_slots_aggregate = LastVotedForkSlotsAggregate::new(root_bank.slot());
    // Aggregate LastVotedForkSlots until seeing this message from 80% of the validators.
    loop {
        let last_voted_fork_slots = cluster_info.get_last_voted_fork_slots(&mut cursor);
        let (slots_to_repair, not_active_percentage) =
            last_voted_fork_slots_aggregate.aggregate(last_voted_fork_slots, &mut epoch_stakes_map);
        if let Some(new_slots) = slots_to_repair {
            let my_bank_forks = bank_forks.read().unwrap();
            if epoch_stakes_map.has_missing_epochs() {
                epoch_stakes_map.update_bank(my_bank_forks.highest_frozen_bank());
            }
            if not_active_percentage < 1.0 - OPTIMISTIC_CONFIRMED_THRESHOLD {
                break;
            }
            let filtered_slots: Vec<Slot> = new_slots
                .into_iter()
                .map(|(slot, _)| slot)
                .filter(|slot| my_bank_forks.bank_hash(slot.clone()).is_none())
                .collect();
            if !filtered_slots.is_empty() {
                if let Err(err) = restart_slots_to_repair_sender.send(Some(filtered_slots)) {
                    error!("Unable to send slots {:?}", err);
                }
            }
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
        let (slots_to_repair, not_active_percentage) =
            last_voted_fork_slots_aggregate.aggregate(Vec::default(), &mut epoch_stakes_map);
        let new_slots = slots_to_repair.unwrap();
        let all_slots_frozen =
            my_bank_forks.are_all_slots_frozen(new_slots.iter().map(|(s, _)| s).collect());
        if all_slots_frozen {
            if let Some((slot, hash, percent)) =
                select_heaviest_fork(new_slots, my_bank_forks, not_active_percentage)
            {
                info!("New Heaviest_Fork message {} {} {}", slot, hash, percent);
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
        HeaviestForkAggregate::new(cluster_info.id(), my_selected_slot, my_selected_hash);
    cursor = Cursor::default();
    loop {
        let heaviest_fork_list = cluster_info.get_heaviest_fork(&mut cursor);
        if heaviest_fork_aggregate.aggregate(heaviest_fork_list, &mut epoch_stakes_map) > 0.8 {
            info!(
                "Success, agreed on {} {}",
                my_selected_slot, my_selected_hash
            );
            break;
        }
        sleep(Duration::from_millis(LISTEN_INTERVAL_MS));
    }
    restart_slots_to_repair_sender.send(None)?;
    let mut my_bank_forks = bank_forks.write().unwrap();
    my_bank_forks.set_root(
        my_selected_slot,
        &accounts_background_request_sender,
        None,
        true,
    );
    loop {
        if get_incremental_snapshot_archives(&snapshot_config.incremental_snapshot_archives_dir)
            .iter()
            .any(|archive_info| archive_info.slot() == my_selected_slot)
        {
            break;
        }
        sleep(Duration::from_millis(LISTEN_INTERVAL_MS));
    }
    Ok(my_selected_slot)
}

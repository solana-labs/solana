use crate::{
    cluster_info_vote_listener::GossipConfirmedSlotsReceiver, fork_choice::ForkChoice,
    heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice, progress_map::ProgressMap,
};
use solana_sdk::{clock::Slot, hash::Hash};
use std::collections::{BTreeMap, HashMap, HashSet};

pub type GossipConfirmedSlots = BTreeMap<Slot, Hash>;
type SlotStateHandler = fn(
    Slot,
    Hash,
    Option<&Hash>,
    bool,
    bool,
    &HashMap<Slot, HashSet<Slot>>,
    &mut ProgressMap,
    &mut HeaviestSubtreeForkChoice,
) -> ();

#[derive(PartialEq, Debug)]
pub enum SlotStateUpdate {
    Frozen,
    Confirmed,
    Dead,
    Duplicate,
}

impl SlotStateUpdate {
    fn to_handler(&self) -> SlotStateHandler {
        match self {
            SlotStateUpdate::Dead => on_dead_slot,
            SlotStateUpdate::Frozen => on_frozen_slot,
            SlotStateUpdate::Confirmed => on_cluster_confirmed_slot,
            SlotStateUpdate::Duplicate => on_duplicate_slot,
        }
    }
}

fn repair_correct_version(slot: Slot, hash: &Hash) {}

fn on_dead_slot(
    slot: Slot,
    bank_frozen_hash: Hash,
    cluster_confirmed_hash: Option<&Hash>,
    is_slot_duplicate: bool,
    is_dead: bool,
    descendants: &HashMap<Slot, HashSet<Slot>>,
    progress: &mut ProgressMap,
    fork_choice: &mut HeaviestSubtreeForkChoice,
) {
    assert!(is_dead);
    // Bank should not have been frozen if the slot was marked dead
    assert_eq!(bank_frozen_hash, Hash::default());
    if let Some(cluster_confirmed_hash) = cluster_confirmed_hash {
        // If the cluster confirmed some version of this slot, then
        // there's another version
        warn!(
            "Cluster confirmed slot {} with hash {}, but we marked slot dead",
            slot, cluster_confirmed_hash
        );
        repair_correct_version(slot, cluster_confirmed_hash);
        progress.set_unconfirmed_duplicate_slot(
            slot,
            descendants.get(&slot).unwrap_or(&HashSet::default()),
        );
    }

    // No need to modify fork choice as dead slots are never frozen,
    // and thus never added to fork choice
}

fn on_frozen_slot(
    slot: Slot,
    bank_frozen_hash: Hash,
    cluster_confirmed_hash: Option<&Hash>,
    mut is_slot_duplicate: bool,
    is_dead: bool,
    descendants: &HashMap<Slot, HashSet<Slot>>,
    progress: &mut ProgressMap,
    fork_choice: &mut HeaviestSubtreeForkChoice,
) {
    // If a slot is marked frozen, the bank hash should not be default,
    // and the slot should not be dead
    assert!(bank_frozen_hash != Hash::default());
    assert!(!is_dead);

    if let Some(cluster_confirmed_hash) = cluster_confirmed_hash {
        // If the cluster confirmed some version of this slot, then
        // confirm our version agrees with the cluster,
        if *cluster_confirmed_hash != bank_frozen_hash {
            // If the versions do not match, modify fork choice rule
            // to exclude our version from being voted on and also
            // repair correct version
            fork_choice.mark_slot_invalid_candidate(slot);
            warn!(
                "Cluster confirmed slot {} with hash {}, but we froze slot with hash {}",
                slot, cluster_confirmed_hash, bank_frozen_hash
            );
            repair_correct_version(slot, cluster_confirmed_hash);
            is_slot_duplicate = true;
        } else {
            // If the versions match, then add the slot to the candidate
            // set to account for the case where it was removed earlier
            // by the `on_duplicate_slot()` handler
            fork_choice.mark_slots_valid_candidate(&[slot]);
            return;
        }
    }

    if is_slot_duplicate {
        // If we detected a duplicate, but have not yet seen any version
        // of the slot confirmed, then remove the slot from fork
        // choice until we get confirmation.
        fork_choice.mark_slot_invalid_candidate(slot);

        // If we get here, we either detected duplicate from
        // 1) WindowService
        // 2) A gossip confirmed version that didn't match our frozen
        // version.
        // In both cases, mark the progress map for this slot as duplicate
        progress.set_unconfirmed_duplicate_slot(
            slot,
            descendants.get(&slot).unwrap_or(&HashSet::default()),
        );
    }
}

// Called when either a slot is confirmed by local replay, or confirmation
// is detected via monitoring gossip
fn on_cluster_confirmed_slot(
    slot: Slot,
    bank_frozen_hash: Hash,
    cluster_confirmed_hash: Option<&Hash>,
    is_slot_duplicate: bool,
    is_dead: bool,
    descendants: &HashMap<Slot, HashSet<Slot>>,
    progress: &mut ProgressMap,
    fork_choice: &mut HeaviestSubtreeForkChoice,
) {
    // This handler should only be called if the cluster actually
    // confirmed some version of the slot
    assert!(cluster_confirmed_hash.is_some());

    // If the slot is dead, and the cluster has confirmed some non-dead version,
    // then call the dead slot handler
    if is_dead {
        on_dead_slot(
            slot,
            bank_frozen_hash,
            cluster_confirmed_hash,
            is_slot_duplicate,
            is_dead,
            descendants,
            progress,
            fork_choice,
        );
    } else if bank_frozen_hash != Hash::default() {
        // If the bank was actually frozen, then check our version
        // agrees with any cluster confirmed version through calling
        // on_frozen_slot()
        on_frozen_slot(
            slot,
            bank_frozen_hash,
            cluster_confirmed_hash,
            is_slot_duplicate,
            is_dead,
            descendants,
            progress,
            fork_choice,
        );
    }
}

// Called when we receive a duplicate slot signal from
// WindowService
fn on_duplicate_slot(
    slot: Slot,
    bank_frozen_hash: Hash,
    cluster_confirmed_hash: Option<&Hash>,
    is_slot_duplicate: bool,
    is_dead: bool,
    descendants: &HashMap<Slot, HashSet<Slot>>,
    progress: &mut ProgressMap,
    fork_choice: &mut HeaviestSubtreeForkChoice,
) {
    assert!(is_slot_duplicate);
    if is_dead {
        on_dead_slot(
            slot,
            bank_frozen_hash,
            cluster_confirmed_hash,
            is_slot_duplicate,
            is_dead,
            descendants,
            progress,
            fork_choice,
        );
    }
    if bank_frozen_hash != Hash::default() {
        // If we have finished replaying this slot, then call the `on_frozen_slot()` handler
        on_frozen_slot(
            slot,
            bank_frozen_hash,
            cluster_confirmed_hash,
            is_slot_duplicate,
            is_dead,
            descendants,
            progress,
            fork_choice,
        );
    }
}

pub fn get_cluster_confirmed_hash<'a>(
    gossip_confirmed_hash: Option<&'a Hash>,
    local_frozen_hash: &'a Hash,
    is_local_replay_confirmed: bool,
) -> Option<&'a Hash> {
    let local_confirmed_hash = if is_local_replay_confirmed {
        // If local replay has confirmed this slot, this slot must have
        // descendants with votes for this slot, hence this slot must be
        // frozen.
        assert!(*local_frozen_hash != Hash::default());
        Some(local_frozen_hash)
    } else {
        None
    };

    match (local_confirmed_hash, gossip_confirmed_hash) {
        (Some(local_frozen_hash), Some(gossip_confirmed_hash)) => {
            assert_eq!(local_frozen_hash, gossip_confirmed_hash);
            Some(&local_frozen_hash)
        }
        (Some(local_frozen_hash), None) => Some(local_frozen_hash),
        _ => gossip_confirmed_hash,
    }
}

pub(crate) fn check_slot_agrees_with_cluster(
    slot: Slot,
    root: Slot,
    frozen_hash: Option<Hash>,
    gossip_confirmed_slots: &GossipConfirmedSlots,
    descendants: &HashMap<Slot, HashSet<Slot>>,
    progress: &mut ProgressMap,
    fork_choice: &mut HeaviestSubtreeForkChoice,
    slot_state_update: SlotStateUpdate,
) {
    if slot <= root {
        return;
    }

    if frozen_hash.is_none() {
        // If the bank doesn't even exist in BankForks yet,
        // then there's nothing to do as replay of the slot
        // hasn't even started
        return;
    }

    let frozen_hash = frozen_hash.unwrap();
    let gossip_confirmed_hash = gossip_confirmed_slots.get(&slot);
    let is_local_replay_confirmed = progress.is_confirmed(slot).expect("If the frozen hash exists, then the slot must exist in bank forks and thus in progress map");
    let cluster_confirmed_hash = get_cluster_confirmed_hash(
        gossip_confirmed_hash,
        &frozen_hash,
        is_local_replay_confirmed,
    );
    let is_slot_duplicate = matches!(slot_state_update, SlotStateUpdate::Duplicate) ||
        progress.is_unconfirmed_duplicate(slot).expect("If the frozen hash exists, then the slot must exist in bank forks and thus in progress map");
    let is_dead = progress.is_dead(slot).expect("If the frozen hash exists, then the slot must exist in bank forks and thus in progress map");

    let state_handler = slot_state_update.to_handler();
    state_handler(
        slot,
        frozen_hash,
        cluster_confirmed_hash,
        is_slot_duplicate,
        is_dead,
        descendants,
        progress,
        fork_choice,
    );
}

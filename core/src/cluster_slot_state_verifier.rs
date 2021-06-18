use crate::{
    fork_choice::ForkChoice, heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
    progress_map::ProgressMap,
};
use solana_sdk::{clock::Slot, hash::Hash};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) type DuplicateSlotsTracker = BTreeSet<Slot>;
pub(crate) type GossipDuplicateConfirmedSlots = BTreeMap<Slot, Hash>;
type SlotStateHandler = fn(Slot, &Hash, Option<&Hash>, bool, bool) -> Vec<ResultingStateChange>;

#[derive(PartialEq, Debug)]
pub enum SlotStateUpdate {
    Frozen,
    DuplicateConfirmed,
    Dead,
    Duplicate,
}

#[derive(PartialEq, Debug)]
pub enum ResultingStateChange {
    // Hash of our current frozen version of the slot
    MarkSlotDuplicate(Hash),
    // Hash of the cluster confirmed slot that is not equivalent
    // to our frozen version of the slot
    RepairDuplicateConfirmedVersion(Hash),
    // Hash of our current frozen version of the slot
    DuplicateConfirmedSlotMatchesCluster(Hash),
}

impl SlotStateUpdate {
    fn to_handler(&self) -> SlotStateHandler {
        match self {
            SlotStateUpdate::Dead => on_dead_slot,
            SlotStateUpdate::Frozen => on_frozen_slot,
            SlotStateUpdate::DuplicateConfirmed => on_cluster_update,
            SlotStateUpdate::Duplicate => on_cluster_update,
        }
    }
}

fn repair_correct_version(_slot: Slot, _hash: &Hash) {}

fn on_dead_slot(
    slot: Slot,
    bank_frozen_hash: &Hash,
    cluster_duplicate_confirmed_hash: Option<&Hash>,
    _is_slot_duplicate: bool,
    is_dead: bool,
) -> Vec<ResultingStateChange> {
    assert!(is_dead);
    // Bank should not have been frozen if the slot was marked dead
    assert_eq!(*bank_frozen_hash, Hash::default());
    if let Some(cluster_duplicate_confirmed_hash) = cluster_duplicate_confirmed_hash {
        // If the cluster duplicate_confirmed some version of this slot, then
        // there's another version
        warn!(
            "Cluster duplicate_confirmed slot {} with hash {}, but we marked slot dead",
            slot, cluster_duplicate_confirmed_hash
        );
        // No need to check `is_slot_duplicate` and modify fork choice as dead slots
        // are never frozen, and thus never added to fork choice. The state change for
        // `MarkSlotDuplicate` will try to modify fork choice, but won't find the slot
        // in the fork choice tree, so is equivalent to a no-op
        return vec![
            ResultingStateChange::MarkSlotDuplicate(Hash::default()),
            ResultingStateChange::RepairDuplicateConfirmedVersion(
                *cluster_duplicate_confirmed_hash,
            ),
        ];
    }

    vec![]
}

fn on_frozen_slot(
    slot: Slot,
    bank_frozen_hash: &Hash,
    cluster_duplicate_confirmed_hash: Option<&Hash>,
    is_slot_duplicate: bool,
    is_dead: bool,
) -> Vec<ResultingStateChange> {
    // If a slot is marked frozen, the bank hash should not be default,
    // and the slot should not be dead
    assert!(*bank_frozen_hash != Hash::default());
    assert!(!is_dead);

    if let Some(cluster_duplicate_confirmed_hash) = cluster_duplicate_confirmed_hash {
        // If the cluster duplicate_confirmed some version of this slot, then
        // confirm our version agrees with the cluster,
        if cluster_duplicate_confirmed_hash != bank_frozen_hash {
            // If the versions do not match, modify fork choice rule
            // to exclude our version from being voted on and also
            // repair correct version
            warn!(
                "Cluster duplicate_confirmed slot {} with hash {}, but we froze slot with hash {}",
                slot, cluster_duplicate_confirmed_hash, bank_frozen_hash
            );
            return vec![
                ResultingStateChange::MarkSlotDuplicate(*bank_frozen_hash),
                ResultingStateChange::RepairDuplicateConfirmedVersion(
                    *cluster_duplicate_confirmed_hash,
                ),
            ];
        } else {
            // If the versions match, then add the slot to the candidate
            // set to account for the case where it was removed earlier
            // by the `on_duplicate_slot()` handler
            return vec![ResultingStateChange::DuplicateConfirmedSlotMatchesCluster(
                *bank_frozen_hash,
            )];
        }
    }

    if is_slot_duplicate {
        // If we detected a duplicate, but have not yet seen any version
        // of the slot duplicate_confirmed (i.e. block above did not execute), then
        // remove the slot from fork choice until we get confirmation.

        // If we get here, we either detected duplicate from
        // 1) WindowService
        // 2) A gossip duplicate_confirmed version that didn't match our frozen
        // version.
        // In both cases, mark the progress map for this slot as duplicate
        return vec![ResultingStateChange::MarkSlotDuplicate(*bank_frozen_hash)];
    }

    vec![]
}

// Called when we receive either:
// 1) A duplicate slot signal from WindowStage,
// 2) Confirmation of a slot by observing votes from replay or gossip.
//
// This signals external information about this slot, which affects
// this validator's understanding of the validity of this slot
fn on_cluster_update(
    slot: Slot,
    bank_frozen_hash: &Hash,
    cluster_duplicate_confirmed_hash: Option<&Hash>,
    is_slot_duplicate: bool,
    is_dead: bool,
) -> Vec<ResultingStateChange> {
    if is_dead {
        on_dead_slot(
            slot,
            bank_frozen_hash,
            cluster_duplicate_confirmed_hash,
            is_slot_duplicate,
            is_dead,
        )
    } else if *bank_frozen_hash != Hash::default() {
        // This case is mutually exclusive with is_dead case above because if a slot is dead,
        // it cannot have  been frozen, and thus cannot have a non-default bank hash.
        on_frozen_slot(
            slot,
            bank_frozen_hash,
            cluster_duplicate_confirmed_hash,
            is_slot_duplicate,
            is_dead,
        )
    } else {
        vec![]
    }
}

fn get_cluster_duplicate_confirmed_hash<'a>(
    slot: Slot,
    gossip_duplicate_confirmed_hash: Option<&'a Hash>,
    local_frozen_hash: &'a Hash,
    is_local_replay_duplicate_confirmed: bool,
) -> Option<&'a Hash> {
    let local_duplicate_confirmed_hash = if is_local_replay_duplicate_confirmed {
        // If local replay has duplicate_confirmed this slot, this slot must have
        // descendants with votes for this slot, hence this slot must be
        // frozen.
        assert!(*local_frozen_hash != Hash::default());
        Some(local_frozen_hash)
    } else {
        None
    };

    match (
        local_duplicate_confirmed_hash,
        gossip_duplicate_confirmed_hash,
    ) {
        (Some(local_duplicate_confirmed_hash), Some(gossip_duplicate_confirmed_hash)) => {
            if local_duplicate_confirmed_hash != gossip_duplicate_confirmed_hash {
                error!(
                    "For slot {}, the gossip duplicate confirmed hash {}, is not equal
                to the confirmed hash we replayed: {}",
                    slot, gossip_duplicate_confirmed_hash, local_duplicate_confirmed_hash
                );
            }
            Some(local_frozen_hash)
        }
        (Some(local_frozen_hash), None) => Some(local_frozen_hash),
        _ => gossip_duplicate_confirmed_hash,
    }
}

fn apply_state_changes(
    slot: Slot,
    fork_choice: &mut HeaviestSubtreeForkChoice,
    state_changes: Vec<ResultingStateChange>,
) {
    for state_change in state_changes {
        match state_change {
            ResultingStateChange::MarkSlotDuplicate(bank_frozen_hash) => {
                fork_choice.mark_fork_invalid_candidate(&(slot, bank_frozen_hash));
            }
            ResultingStateChange::RepairDuplicateConfirmedVersion(
                cluster_duplicate_confirmed_hash,
            ) => {
                // TODO: Should consider moving the updating of the duplicate slots in the
                // progress map from ReplayStage::confirm_forks to here.
                repair_correct_version(slot, &cluster_duplicate_confirmed_hash);
            }
            ResultingStateChange::DuplicateConfirmedSlotMatchesCluster(bank_frozen_hash) => {
                fork_choice.mark_fork_valid_candidate(&(slot, bank_frozen_hash));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn check_slot_agrees_with_cluster(
    slot: Slot,
    root: Slot,
    frozen_hash: Option<Hash>,
    duplicate_slots_tracker: &mut DuplicateSlotsTracker,
    gossip_duplicate_confirmed_slots: &GossipDuplicateConfirmedSlots,
    progress: &ProgressMap,
    fork_choice: &mut HeaviestSubtreeForkChoice,
    slot_state_update: SlotStateUpdate,
) {
    info!(
        "check_slot_agrees_with_cluster()
        slot: {},
        root: {},
        frozen_hash: {:?},
        update: {:?}",
        slot, root, frozen_hash, slot_state_update
    );

    if slot <= root {
        return;
    }

    // Needs to happen before the frozen_hash.is_none() check below to account for duplicate
    // signals arriving before the bank is constructed in replay.
    if matches!(slot_state_update, SlotStateUpdate::Duplicate) {
        // If this slot has already been processed before, return
        if !duplicate_slots_tracker.insert(slot) {
            return;
        }
    }

    if frozen_hash.is_none() {
        // If the bank doesn't even exist in BankForks yet,
        // then there's nothing to do as replay of the slot
        // hasn't even started
        return;
    }

    let frozen_hash = frozen_hash.unwrap();
    let gossip_duplicate_confirmed_hash = gossip_duplicate_confirmed_slots.get(&slot);

    // If the bank hasn't been frozen yet, then we haven't duplicate confirmed a local version
    // this slot through replay yet.
    let is_local_replay_duplicate_confirmed = fork_choice
        .is_duplicate_confirmed(&(slot, frozen_hash))
        .unwrap_or(false);
    let cluster_duplicate_confirmed_hash = get_cluster_duplicate_confirmed_hash(
        slot,
        gossip_duplicate_confirmed_hash,
        &frozen_hash,
        is_local_replay_duplicate_confirmed,
    );
    let is_slot_duplicate = duplicate_slots_tracker.contains(&slot);
    let is_dead = progress.is_dead(slot).expect("If the frozen hash exists, then the slot must exist in bank forks and thus in progress map");

    info!(
        "check_slot_agrees_with_cluster() state
        is_local_replay_duplicate_confirmed: {:?},
        cluster_duplicate_confirmed_hash: {:?},
        is_slot_duplicate: {:?},
        is_dead: {:?}",
        is_local_replay_duplicate_confirmed,
        cluster_duplicate_confirmed_hash,
        is_slot_duplicate,
        is_dead,
    );

    let state_handler = slot_state_update.to_handler();
    let state_changes = state_handler(
        slot,
        &frozen_hash,
        cluster_duplicate_confirmed_hash,
        is_slot_duplicate,
        is_dead,
    );
    apply_state_changes(slot, fork_choice, state_changes);
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::consensus::test::VoteSimulator;
    use solana_runtime::bank_forks::BankForks;
    use std::{
        collections::{HashMap, HashSet},
        sync::RwLock,
    };
    use trees::tr;

    struct InitialState {
        heaviest_subtree_fork_choice: HeaviestSubtreeForkChoice,
        progress: ProgressMap,
        descendants: HashMap<Slot, HashSet<Slot>>,
        bank_forks: RwLock<BankForks>,
    }

    fn setup() -> InitialState {
        // Create simple fork 0 -> 1 -> 2 -> 3
        let forks = tr(0) / (tr(1) / (tr(2) / tr(3)));
        let mut vote_simulator = VoteSimulator::new(1);
        vote_simulator.fill_bank_forks(forks, &HashMap::new());

        let descendants = vote_simulator
            .bank_forks
            .read()
            .unwrap()
            .descendants()
            .clone();

        InitialState {
            heaviest_subtree_fork_choice: vote_simulator.heaviest_subtree_fork_choice,
            progress: vote_simulator.progress,
            descendants,
            bank_forks: vote_simulator.bank_forks,
        }
    }

    #[test]
    fn test_frozen_duplicate() {
        // Common state
        let slot = 0;
        let cluster_duplicate_confirmed_hash = None;
        let is_dead = false;

        // Slot is not detected as duplicate yet
        let mut is_slot_duplicate = false;

        // Simulate freezing the bank, add a
        // new non-default hash, should return
        // no actionable state changes yet
        let bank_frozen_hash = Hash::new_unique();
        assert!(on_frozen_slot(
            slot,
            &bank_frozen_hash,
            cluster_duplicate_confirmed_hash,
            is_slot_duplicate,
            is_dead
        )
        .is_empty());

        // Now mark the slot as duplicate, should
        // trigger marking the slot as a duplicate
        is_slot_duplicate = true;
        assert_eq!(
            on_cluster_update(
                slot,
                &bank_frozen_hash,
                cluster_duplicate_confirmed_hash,
                is_slot_duplicate,
                is_dead
            ),
            vec![ResultingStateChange::MarkSlotDuplicate(bank_frozen_hash)]
        );
    }

    #[test]
    fn test_frozen_duplicate_confirmed() {
        // Common state
        let slot = 0;
        let is_slot_duplicate = false;
        let is_dead = false;

        // No cluster duplicate_confirmed hash yet
        let mut cluster_duplicate_confirmed_hash = None;

        // Simulate freezing the bank, add a
        // new non-default hash, should return
        // no actionable state changes
        let bank_frozen_hash = Hash::new_unique();
        assert!(on_frozen_slot(
            slot,
            &bank_frozen_hash,
            cluster_duplicate_confirmed_hash,
            is_slot_duplicate,
            is_dead
        )
        .is_empty());

        // Now mark the same frozen slot hash as duplicate_confirmed by the cluster,
        // should just confirm the slot
        cluster_duplicate_confirmed_hash = Some(&bank_frozen_hash);
        assert_eq!(
            on_cluster_update(
                slot,
                &bank_frozen_hash,
                cluster_duplicate_confirmed_hash,
                is_slot_duplicate,
                is_dead
            ),
            vec![ResultingStateChange::DuplicateConfirmedSlotMatchesCluster(
                bank_frozen_hash
            ),]
        );

        // If the cluster_duplicate_confirmed_hash does not match, then we
        // should trigger marking the slot as a duplicate, and also
        // try to repair correct version
        let mismatched_hash = Hash::new_unique();
        cluster_duplicate_confirmed_hash = Some(&mismatched_hash);
        assert_eq!(
            on_cluster_update(
                slot,
                &bank_frozen_hash,
                cluster_duplicate_confirmed_hash,
                is_slot_duplicate,
                is_dead
            ),
            vec![
                ResultingStateChange::MarkSlotDuplicate(bank_frozen_hash),
                ResultingStateChange::RepairDuplicateConfirmedVersion(mismatched_hash),
            ]
        );
    }

    #[test]
    fn test_duplicate_frozen_duplicate_confirmed() {
        // Common state
        let slot = 0;
        let is_dead = false;
        let is_slot_duplicate = true;

        // Bank is not frozen yet
        let mut cluster_duplicate_confirmed_hash = None;
        let mut bank_frozen_hash = Hash::default();

        // Mark the slot as duplicate. Because our version of the slot is not
        // frozen yet, we don't know which version we have, so no action is
        // taken.
        assert!(on_cluster_update(
            slot,
            &bank_frozen_hash,
            cluster_duplicate_confirmed_hash,
            is_slot_duplicate,
            is_dead
        )
        .is_empty());

        // Freeze the bank, should now mark the slot as duplicate since we have
        // not seen confirmation yet.
        bank_frozen_hash = Hash::new_unique();
        assert_eq!(
            on_cluster_update(
                slot,
                &bank_frozen_hash,
                cluster_duplicate_confirmed_hash,
                is_slot_duplicate,
                is_dead
            ),
            vec![ResultingStateChange::MarkSlotDuplicate(bank_frozen_hash),]
        );

        // If the cluster_duplicate_confirmed_hash matches, we just confirm
        // the slot
        cluster_duplicate_confirmed_hash = Some(&bank_frozen_hash);
        assert_eq!(
            on_cluster_update(
                slot,
                &bank_frozen_hash,
                cluster_duplicate_confirmed_hash,
                is_slot_duplicate,
                is_dead
            ),
            vec![ResultingStateChange::DuplicateConfirmedSlotMatchesCluster(
                bank_frozen_hash
            ),]
        );

        // If the cluster_duplicate_confirmed_hash does not match, then we
        // should trigger marking the slot as a duplicate, and also
        // try to repair correct version
        let mismatched_hash = Hash::new_unique();
        cluster_duplicate_confirmed_hash = Some(&mismatched_hash);
        assert_eq!(
            on_cluster_update(
                slot,
                &bank_frozen_hash,
                cluster_duplicate_confirmed_hash,
                is_slot_duplicate,
                is_dead
            ),
            vec![
                ResultingStateChange::MarkSlotDuplicate(bank_frozen_hash),
                ResultingStateChange::RepairDuplicateConfirmedVersion(mismatched_hash),
            ]
        );
    }

    #[test]
    fn test_duplicate_duplicate_confirmed() {
        let slot = 0;
        let correct_hash = Hash::new_unique();
        let cluster_duplicate_confirmed_hash = Some(&correct_hash);
        let is_dead = false;
        // Bank is not frozen yet
        let bank_frozen_hash = Hash::default();

        // Because our version of the slot is not frozen yet, then even though
        // the cluster has duplicate_confirmed a hash, we don't know which version we
        // have, so no action is taken.
        let is_slot_duplicate = true;
        assert!(on_cluster_update(
            slot,
            &bank_frozen_hash,
            cluster_duplicate_confirmed_hash,
            is_slot_duplicate,
            is_dead
        )
        .is_empty());
    }

    #[test]
    fn test_duplicate_dead() {
        let slot = 0;
        let cluster_duplicate_confirmed_hash = None;
        let is_dead = true;
        // Bank is not frozen yet
        let bank_frozen_hash = Hash::default();

        // Even though our version of the slot is dead, the cluster has not
        // duplicate_confirmed a hash, we don't know which version we have, so no action
        // is taken.
        let is_slot_duplicate = true;
        assert!(on_cluster_update(
            slot,
            &bank_frozen_hash,
            cluster_duplicate_confirmed_hash,
            is_slot_duplicate,
            is_dead
        )
        .is_empty());
    }

    #[test]
    fn test_duplicate_confirmed_dead_duplicate() {
        let slot = 0;
        let correct_hash = Hash::new_unique();
        // Cluster has duplicate_confirmed some version of the slot
        let cluster_duplicate_confirmed_hash = Some(&correct_hash);
        // Our version of the slot is dead
        let is_dead = true;
        let bank_frozen_hash = Hash::default();

        // Even if the duplicate signal hasn't come in yet,
        // we can deduce the slot is duplicate AND we have,
        // the wrong version, so should mark the slot as duplicate,
        // and repair the correct version
        let mut is_slot_duplicate = false;
        assert_eq!(
            on_cluster_update(
                slot,
                &bank_frozen_hash,
                cluster_duplicate_confirmed_hash,
                is_slot_duplicate,
                is_dead
            ),
            vec![
                ResultingStateChange::MarkSlotDuplicate(bank_frozen_hash),
                ResultingStateChange::RepairDuplicateConfirmedVersion(correct_hash),
            ]
        );

        // If the duplicate signal comes in, nothing should change
        is_slot_duplicate = true;
        assert_eq!(
            on_cluster_update(
                slot,
                &bank_frozen_hash,
                cluster_duplicate_confirmed_hash,
                is_slot_duplicate,
                is_dead
            ),
            vec![
                ResultingStateChange::MarkSlotDuplicate(bank_frozen_hash),
                ResultingStateChange::RepairDuplicateConfirmedVersion(correct_hash),
            ]
        );
    }

    #[test]
    fn test_apply_state_changes() {
        // Common state
        let InitialState {
            mut heaviest_subtree_fork_choice,
            descendants,
            bank_forks,
            ..
        } = setup();

        // MarkSlotDuplicate should mark progress map and remove
        // the slot from fork choice
        let duplicate_slot = bank_forks.read().unwrap().root() + 1;
        let duplicate_slot_hash = bank_forks
            .read()
            .unwrap()
            .get(duplicate_slot)
            .unwrap()
            .hash();
        apply_state_changes(
            duplicate_slot,
            &mut heaviest_subtree_fork_choice,
            vec![ResultingStateChange::MarkSlotDuplicate(duplicate_slot_hash)],
        );
        assert!(!heaviest_subtree_fork_choice
            .is_candidate(&(duplicate_slot, duplicate_slot_hash))
            .unwrap());
        for child_slot in descendants
            .get(&duplicate_slot)
            .unwrap()
            .iter()
            .chain(std::iter::once(&duplicate_slot))
        {
            assert_eq!(
                heaviest_subtree_fork_choice
                    .latest_invalid_ancestor(&(
                        *child_slot,
                        bank_forks.read().unwrap().get(*child_slot).unwrap().hash()
                    ))
                    .unwrap(),
                duplicate_slot
            );
        }

        // DuplicateConfirmedSlotMatchesCluster should re-enable fork choice
        apply_state_changes(
            duplicate_slot,
            &mut heaviest_subtree_fork_choice,
            vec![ResultingStateChange::DuplicateConfirmedSlotMatchesCluster(
                duplicate_slot_hash,
            )],
        );
        for child_slot in descendants
            .get(&duplicate_slot)
            .unwrap()
            .iter()
            .chain(std::iter::once(&duplicate_slot))
        {
            assert!(heaviest_subtree_fork_choice
                .latest_invalid_ancestor(&(
                    *child_slot,
                    bank_forks.read().unwrap().get(*child_slot).unwrap().hash()
                ))
                .is_none());
        }
        assert!(heaviest_subtree_fork_choice
            .is_candidate(&(duplicate_slot, duplicate_slot_hash))
            .unwrap());
    }

    fn run_test_state_duplicate_then_bank_frozen(initial_bank_hash: Option<Hash>) {
        // Common state
        let InitialState {
            mut heaviest_subtree_fork_choice,
            progress,
            bank_forks,
            ..
        } = setup();

        // Setup a duplicate slot state transition with the initial bank state of the duplicate slot
        // determined by `initial_bank_hash`, which can be:
        // 1) A default hash (unfrozen bank),
        // 2) None (a slot that hasn't even started replay yet).
        let root = 0;
        let mut duplicate_slots_tracker = DuplicateSlotsTracker::default();
        let gossip_duplicate_confirmed_slots = GossipDuplicateConfirmedSlots::default();
        let duplicate_slot = 2;
        check_slot_agrees_with_cluster(
            duplicate_slot,
            root,
            initial_bank_hash,
            &mut duplicate_slots_tracker,
            &gossip_duplicate_confirmed_slots,
            &progress,
            &mut heaviest_subtree_fork_choice,
            SlotStateUpdate::Duplicate,
        );
        assert!(duplicate_slots_tracker.contains(&duplicate_slot));
        // Nothing should be applied yet to fork choice, since bank was not yet frozen
        for slot in 2..=3 {
            let slot_hash = bank_forks.read().unwrap().get(slot).unwrap().hash();
            assert!(heaviest_subtree_fork_choice
                .latest_invalid_ancestor(&(slot, slot_hash))
                .is_none());
        }

        // Now freeze the bank
        let frozen_duplicate_slot_hash = bank_forks
            .read()
            .unwrap()
            .get(duplicate_slot)
            .unwrap()
            .hash();
        check_slot_agrees_with_cluster(
            duplicate_slot,
            root,
            Some(frozen_duplicate_slot_hash),
            &mut duplicate_slots_tracker,
            &gossip_duplicate_confirmed_slots,
            &progress,
            &mut heaviest_subtree_fork_choice,
            SlotStateUpdate::Frozen,
        );

        // Progress map should have the correct updates, fork choice should mark duplicate
        // as unvotable
        assert!(heaviest_subtree_fork_choice
            .is_unconfirmed_duplicate(&(duplicate_slot, frozen_duplicate_slot_hash))
            .unwrap());

        // The ancestor of the duplicate slot should be the best slot now
        let (duplicate_ancestor, duplicate_parent_hash) = {
            let r_bank_forks = bank_forks.read().unwrap();
            let parent_bank = r_bank_forks.get(duplicate_slot).unwrap().parent().unwrap();
            (parent_bank.slot(), parent_bank.hash())
        };
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            (duplicate_ancestor, duplicate_parent_hash)
        );
    }

    #[test]
    fn test_state_unfrozen_bank_duplicate_then_bank_frozen() {
        run_test_state_duplicate_then_bank_frozen(Some(Hash::default()));
    }

    #[test]
    fn test_state_unreplayed_bank_duplicate_then_bank_frozen() {
        run_test_state_duplicate_then_bank_frozen(None);
    }

    #[test]
    fn test_state_ancestor_confirmed_descendant_duplicate() {
        // Common state
        let InitialState {
            mut heaviest_subtree_fork_choice,
            progress,
            bank_forks,
            ..
        } = setup();

        let slot3_hash = bank_forks.read().unwrap().get(3).unwrap().hash();
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            (3, slot3_hash)
        );
        let root = 0;
        let mut duplicate_slots_tracker = DuplicateSlotsTracker::default();
        let mut gossip_duplicate_confirmed_slots = GossipDuplicateConfirmedSlots::default();

        // Mark slot 2 as duplicate confirmed
        let slot2_hash = bank_forks.read().unwrap().get(2).unwrap().hash();
        gossip_duplicate_confirmed_slots.insert(2, slot2_hash);
        check_slot_agrees_with_cluster(
            2,
            root,
            Some(slot2_hash),
            &mut duplicate_slots_tracker,
            &gossip_duplicate_confirmed_slots,
            &progress,
            &mut heaviest_subtree_fork_choice,
            SlotStateUpdate::DuplicateConfirmed,
        );
        assert!(heaviest_subtree_fork_choice
            .is_duplicate_confirmed(&(2, slot2_hash))
            .unwrap());
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            (3, slot3_hash)
        );
        for slot in 0..=2 {
            let slot_hash = bank_forks.read().unwrap().get(slot).unwrap().hash();
            assert!(heaviest_subtree_fork_choice
                .is_duplicate_confirmed(&(slot, slot_hash))
                .unwrap());
            assert!(heaviest_subtree_fork_choice
                .latest_invalid_ancestor(&(slot, slot_hash))
                .is_none());
        }

        // Mark 3 as duplicate, should not remove the duplicate confirmed slot 2 from
        // fork choice
        check_slot_agrees_with_cluster(
            3,
            root,
            Some(slot3_hash),
            &mut duplicate_slots_tracker,
            &gossip_duplicate_confirmed_slots,
            &progress,
            &mut heaviest_subtree_fork_choice,
            SlotStateUpdate::Duplicate,
        );
        assert!(duplicate_slots_tracker.contains(&3));
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            (2, slot2_hash)
        );
        for slot in 0..=3 {
            let slot_hash = bank_forks.read().unwrap().get(slot).unwrap().hash();
            if slot <= 2 {
                assert!(heaviest_subtree_fork_choice
                    .is_duplicate_confirmed(&(slot, slot_hash))
                    .unwrap());
                assert!(heaviest_subtree_fork_choice
                    .latest_invalid_ancestor(&(slot, slot_hash))
                    .is_none());
            } else {
                assert!(!heaviest_subtree_fork_choice
                    .is_duplicate_confirmed(&(slot, slot_hash))
                    .unwrap());
                assert_eq!(
                    heaviest_subtree_fork_choice
                        .latest_invalid_ancestor(&(slot, slot_hash))
                        .unwrap(),
                    3
                );
            }
        }
    }

    #[test]
    fn test_state_ancestor_duplicate_descendant_confirmed() {
        // Common state
        let InitialState {
            mut heaviest_subtree_fork_choice,
            progress,
            bank_forks,
            ..
        } = setup();

        let slot3_hash = bank_forks.read().unwrap().get(3).unwrap().hash();
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            (3, slot3_hash)
        );
        let root = 0;
        let mut duplicate_slots_tracker = DuplicateSlotsTracker::default();
        let mut gossip_duplicate_confirmed_slots = GossipDuplicateConfirmedSlots::default();

        // Mark 2 as duplicate
        check_slot_agrees_with_cluster(
            2,
            root,
            Some(bank_forks.read().unwrap().get(2).unwrap().hash()),
            &mut duplicate_slots_tracker,
            &gossip_duplicate_confirmed_slots,
            &progress,
            &mut heaviest_subtree_fork_choice,
            SlotStateUpdate::Duplicate,
        );
        assert!(duplicate_slots_tracker.contains(&2));
        for slot in 2..=3 {
            let slot_hash = bank_forks.read().unwrap().get(slot).unwrap().hash();
            assert_eq!(
                heaviest_subtree_fork_choice
                    .latest_invalid_ancestor(&(slot, slot_hash))
                    .unwrap(),
                2
            );
        }

        let slot1_hash = bank_forks.read().unwrap().get(1).unwrap().hash();
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            (1, slot1_hash)
        );

        // Mark slot 3 as duplicate confirmed, should mark slot 2 as duplicate confirmed as well
        gossip_duplicate_confirmed_slots.insert(3, slot3_hash);
        check_slot_agrees_with_cluster(
            3,
            root,
            Some(slot3_hash),
            &mut duplicate_slots_tracker,
            &gossip_duplicate_confirmed_slots,
            &progress,
            &mut heaviest_subtree_fork_choice,
            SlotStateUpdate::DuplicateConfirmed,
        );
        for slot in 0..=3 {
            let slot_hash = bank_forks.read().unwrap().get(slot).unwrap().hash();
            assert!(heaviest_subtree_fork_choice
                .is_duplicate_confirmed(&(slot, slot_hash))
                .unwrap());
            assert!(heaviest_subtree_fork_choice
                .latest_invalid_ancestor(&(slot, slot_hash))
                .is_none());
        }
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            (3, slot3_hash)
        );
    }

    #[test]
    fn test_state_descendant_confirmed_ancestor_duplicate() {
        // Common state
        let InitialState {
            mut heaviest_subtree_fork_choice,
            progress,
            bank_forks,
            ..
        } = setup();

        let slot3_hash = bank_forks.read().unwrap().get(3).unwrap().hash();
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            (3, slot3_hash)
        );
        let root = 0;
        let mut duplicate_slots_tracker = DuplicateSlotsTracker::default();
        let mut gossip_duplicate_confirmed_slots = GossipDuplicateConfirmedSlots::default();

        // Mark 3 as duplicate confirmed
        gossip_duplicate_confirmed_slots.insert(3, slot3_hash);
        check_slot_agrees_with_cluster(
            3,
            root,
            Some(slot3_hash),
            &mut duplicate_slots_tracker,
            &gossip_duplicate_confirmed_slots,
            &progress,
            &mut heaviest_subtree_fork_choice,
            SlotStateUpdate::DuplicateConfirmed,
        );
        let verify_all_slots_duplicate_confirmed =
            |bank_forks: &RwLock<BankForks>,
             heaviest_subtree_fork_choice: &HeaviestSubtreeForkChoice| {
                for slot in 0..=3 {
                    let slot_hash = bank_forks.read().unwrap().get(slot).unwrap().hash();
                    assert!(heaviest_subtree_fork_choice
                        .is_duplicate_confirmed(&(slot, slot_hash))
                        .unwrap());
                    assert!(heaviest_subtree_fork_choice
                        .latest_invalid_ancestor(&(slot, slot_hash))
                        .is_none());
                }
            };
        verify_all_slots_duplicate_confirmed(&bank_forks, &heaviest_subtree_fork_choice);
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            (3, slot3_hash)
        );

        // Mark ancestor 1 as duplicate, fork choice should be unaffected since
        // slot 1 was duplicate confirmed by the confirmation on its
        // descendant, 3.
        let slot1_hash = bank_forks.read().unwrap().get(1).unwrap().hash();
        check_slot_agrees_with_cluster(
            1,
            root,
            Some(slot1_hash),
            &mut duplicate_slots_tracker,
            &gossip_duplicate_confirmed_slots,
            &progress,
            &mut heaviest_subtree_fork_choice,
            SlotStateUpdate::Duplicate,
        );
        assert!(duplicate_slots_tracker.contains(&1));
        verify_all_slots_duplicate_confirmed(&bank_forks, &heaviest_subtree_fork_choice);
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            (3, slot3_hash)
        );
    }
}

use {
    crate::{
        heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice, repair_service::RepairService,
        serve_repair::ShredRepairType, tree_diff::TreeDiff,
    },
    solana_ledger::{blockstore::Blockstore, blockstore_meta::SlotMeta},
    solana_runtime::contains::Contains,
    solana_sdk::{clock::Slot, hash::Hash},
    std::collections::{HashMap, HashSet},
};

#[derive(Debug, PartialEq, Eq)]
enum Visit {
    Visited(Slot),
    Unvisited(Slot),
}

impl Visit {
    pub fn slot(&self) -> Slot {
        match self {
            Visit::Visited(slot) => *slot,
            Visit::Unvisited(slot) => *slot,
        }
    }
}

// Iterates through slots in order of weight
struct RepairWeightTraversal<'a> {
    tree: &'a HeaviestSubtreeForkChoice,
    pending: Vec<Visit>,
}

impl<'a> RepairWeightTraversal<'a> {
    fn new(tree: &'a HeaviestSubtreeForkChoice) -> Self {
        Self {
            tree,
            pending: vec![Visit::Unvisited(tree.tree_root().0)],
        }
    }
}

impl<'a> Iterator for RepairWeightTraversal<'a> {
    type Item = Visit;
    fn next(&mut self) -> Option<Self::Item> {
        let next = self.pending.pop();
        next.map(|next| {
            if let Visit::Unvisited(slot) = next {
                // Add a bookmark to communicate all child
                // slots have been visited
                self.pending.push(Visit::Visited(slot));
                let mut children: Vec<_> = self
                    .tree
                    .children(&(slot, Hash::default()))
                    .unwrap()
                    .map(|(child_slot, _)| Visit::Unvisited(*child_slot))
                    .collect();

                // Sort children by weight to prioritize visiting the heaviest
                // ones first
                children.sort_by(|slot1, slot2| {
                    self.tree.max_by_weight(
                        (slot1.slot(), Hash::default()),
                        (slot2.slot(), Hash::default()),
                    )
                });
                self.pending.extend(children);
            }
            next
        })
    }
}

/// Generate shred repairs for `tree` starting at `tree.root`.
/// Prioritized by stake weight, additionally considers children not present in `tree` but in
/// blockstore.
pub fn get_best_repair_shreds<'a>(
    tree: &HeaviestSubtreeForkChoice,
    blockstore: &Blockstore,
    slot_meta_cache: &mut HashMap<Slot, Option<SlotMeta>>,
    repairs: &mut Vec<ShredRepairType>,
    max_new_shreds: usize,
    ignore_slots: &impl Contains<'a, Slot>,
) {
    let initial_len = repairs.len();
    let max_repairs = initial_len + max_new_shreds;
    let weighted_iter = RepairWeightTraversal::new(tree);
    let mut visited_set = HashSet::new();
    for next in weighted_iter {
        if repairs.len() > max_repairs {
            break;
        }

        let slot_meta = slot_meta_cache
            .entry(next.slot())
            .or_insert_with(|| blockstore.meta(next.slot()).unwrap());

        // May not exist if blockstore purged the SlotMeta due to something
        // like duplicate slots. TODO: Account for duplicate slot may be in orphans, especially
        // if earlier duplicate was already removed
        if let Some(slot_meta) = slot_meta {
            match next {
                Visit::Unvisited(slot) => {
                    if !ignore_slots.contains(&slot) {
                        let new_repairs = RepairService::generate_repairs_for_slot(
                            blockstore,
                            slot,
                            slot_meta,
                            max_repairs - repairs.len(),
                        );
                        repairs.extend(new_repairs);
                    }
                    visited_set.insert(slot);
                }
                Visit::Visited(_) => {
                    // By the time we reach here, this means all the children of this slot
                    // have been explored/repaired. Although this slot has already been visited,
                    // this slot is still the heaviest slot left in the traversal. Thus any
                    // remaining children that have not been explored should now be repaired.
                    for new_child_slot in &slot_meta.next_slots {
                        // If the `new_child_slot` has not been visited by now, it must
                        // not exist in `tree`
                        if !visited_set.contains(new_child_slot) {
                            // Generate repairs for entire subtree rooted at `new_child_slot`
                            RepairService::generate_repairs_for_fork(
                                blockstore,
                                repairs,
                                max_repairs,
                                *new_child_slot,
                                ignore_slots,
                            );
                        }
                        visited_set.insert(*new_child_slot);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        solana_ledger::{
            get_tmp_ledger_path,
            shred::{Shred, ShredFlags},
        },
        solana_runtime::bank_utils,
        solana_sdk::hash::Hash,
        trees::tr,
    };

    #[test]
    fn test_weighted_repair_traversal_single() {
        let heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new((42, Hash::default()));
        let weighted_traversal = RepairWeightTraversal::new(&heaviest_subtree_fork_choice);
        let steps: Vec<_> = weighted_traversal.collect();
        assert_eq!(steps, vec![Visit::Unvisited(42), Visit::Visited(42)]);
    }

    #[test]
    fn test_weighted_repair_traversal() {
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(1, stake);
        let (_, mut heaviest_subtree_fork_choice) = setup_forks();
        let weighted_traversal = RepairWeightTraversal::new(&heaviest_subtree_fork_choice);
        let steps: Vec<_> = weighted_traversal.collect();

        // When every node has a weight of zero, visit
        // smallest children first
        assert_eq!(
            steps,
            vec![
                Visit::Unvisited(0),
                Visit::Unvisited(1),
                Visit::Unvisited(2),
                Visit::Unvisited(4),
                Visit::Visited(4),
                Visit::Visited(2),
                Visit::Unvisited(3),
                Visit::Unvisited(5),
                Visit::Visited(5),
                Visit::Visited(3),
                Visit::Visited(1),
                Visit::Visited(0)
            ]
        );

        // Add a vote to branch with slot 5,
        // should prioritize that branch
        heaviest_subtree_fork_choice.add_votes(
            [(vote_pubkeys[0], (5, Hash::default()))].iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        let weighted_traversal = RepairWeightTraversal::new(&heaviest_subtree_fork_choice);
        let steps: Vec<_> = weighted_traversal.collect();
        assert_eq!(
            steps,
            vec![
                Visit::Unvisited(0),
                Visit::Unvisited(1),
                Visit::Unvisited(3),
                Visit::Unvisited(5),
                Visit::Visited(5),
                // Prioritizes heavier child 3 over 2
                Visit::Visited(3),
                Visit::Unvisited(2),
                Visit::Unvisited(4),
                Visit::Visited(4),
                Visit::Visited(2),
                Visit::Visited(1),
                Visit::Visited(0)
            ]
        );
    }

    #[test]
    fn test_get_best_repair_shreds() {
        let (blockstore, heaviest_subtree_fork_choice) = setup_forks();

        // `blockstore` and `heaviest_subtree_fork_choice` match exactly, so should
        // return repairs for all slots (none are completed) in order of traversal
        let mut repairs = vec![];
        let mut slot_meta_cache = HashMap::default();
        let last_shred = blockstore.meta(0).unwrap().unwrap().received;
        get_best_repair_shreds(
            &heaviest_subtree_fork_choice,
            &blockstore,
            &mut slot_meta_cache,
            &mut repairs,
            6,
            &HashSet::default(),
        );
        assert_eq!(
            repairs,
            [0, 1, 2, 4, 3, 5]
                .iter()
                .map(|slot| ShredRepairType::HighestShred(*slot, last_shred))
                .collect::<Vec<_>>()
        );

        // Add some leaves to blockstore, attached to the current best leaf, should prioritize
        // repairing those new leaves before trying other branches
        repairs = vec![];
        slot_meta_cache = HashMap::default();
        let best_overall_slot = heaviest_subtree_fork_choice.best_overall_slot().0;
        assert_eq!(best_overall_slot, 4);
        blockstore.add_tree(
            tr(best_overall_slot) / (tr(6) / tr(7)),
            true,
            false,
            2,
            Hash::default(),
        );
        get_best_repair_shreds(
            &heaviest_subtree_fork_choice,
            &blockstore,
            &mut slot_meta_cache,
            &mut repairs,
            6,
            &HashSet::default(),
        );
        assert_eq!(
            repairs,
            [0, 1, 2, 4, 6, 7]
                .iter()
                .map(|slot| ShredRepairType::HighestShred(*slot, last_shred))
                .collect::<Vec<_>>()
        );

        // Completing slots should remove them from the repair list
        repairs = vec![];
        slot_meta_cache = HashMap::default();
        let completed_shreds: Vec<Shred> = [0, 2, 4, 6]
            .iter()
            .map(|slot| {
                let parent_offset = u16::from(*slot != 0);
                let shred = Shred::new_from_data(
                    *slot,
                    last_shred as u32, // index
                    parent_offset,
                    &[0u8; 8], // data
                    ShredFlags::LAST_SHRED_IN_SLOT,
                    8,                 // reference_tick
                    0,                 // version
                    last_shred as u32, // fec_set_index
                );
                assert!(shred.sanitize().is_ok());
                shred
            })
            .collect();
        blockstore
            .insert_shreds(completed_shreds, None, false)
            .unwrap();
        get_best_repair_shreds(
            &heaviest_subtree_fork_choice,
            &blockstore,
            &mut slot_meta_cache,
            &mut repairs,
            4,
            &HashSet::default(),
        );
        assert_eq!(
            repairs,
            [1, 7, 3, 5]
                .iter()
                .map(|slot| ShredRepairType::HighestShred(*slot, last_shred))
                .collect::<Vec<_>>()
        );

        // Adding incomplete children with higher weighted parents, even if
        // the parents are complete should still be repaired
        repairs = vec![];
        slot_meta_cache = HashMap::default();
        blockstore.add_tree(tr(2) / (tr(8)), true, false, 2, Hash::default());
        get_best_repair_shreds(
            &heaviest_subtree_fork_choice,
            &blockstore,
            &mut slot_meta_cache,
            &mut repairs,
            4,
            &HashSet::default(),
        );
        assert_eq!(
            repairs,
            [1, 7, 8, 3]
                .iter()
                .map(|slot| ShredRepairType::HighestShred(*slot, last_shred))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_get_best_repair_shreds_no_duplicates() {
        let (blockstore, heaviest_subtree_fork_choice) = setup_forks();
        // Add a branch to slot 2, make sure it doesn't repair child
        // 4 again when the Unvisited(2) event happens
        blockstore.add_tree(tr(2) / (tr(6) / tr(7)), true, false, 2, Hash::default());
        let mut repairs = vec![];
        let mut slot_meta_cache = HashMap::default();
        get_best_repair_shreds(
            &heaviest_subtree_fork_choice,
            &blockstore,
            &mut slot_meta_cache,
            &mut repairs,
            std::usize::MAX,
            &HashSet::default(),
        );
        let last_shred = blockstore.meta(0).unwrap().unwrap().received;
        assert_eq!(
            repairs,
            [0, 1, 2, 4, 6, 7, 3, 5]
                .iter()
                .map(|slot| ShredRepairType::HighestShred(*slot, last_shred))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_get_best_repair_shreds_ignore() {
        let (blockstore, heaviest_subtree_fork_choice) = setup_forks();

        // Adding slots to ignore should remove them from the repair set, but
        // should not remove their children
        let mut repairs = vec![];
        let mut slot_meta_cache = HashMap::default();
        let mut ignore_set: HashSet<Slot> = vec![1, 3].into_iter().collect();
        let last_shred = blockstore.meta(0).unwrap().unwrap().received;
        get_best_repair_shreds(
            &heaviest_subtree_fork_choice,
            &blockstore,
            &mut slot_meta_cache,
            &mut repairs,
            std::usize::MAX,
            &ignore_set,
        );
        assert_eq!(
            repairs,
            [0, 2, 4, 5]
                .iter()
                .map(|slot| ShredRepairType::HighestShred(*slot, last_shred))
                .collect::<Vec<_>>()
        );

        // Adding slot 2 to ignore should not remove its unexplored children from
        // the repair set
        repairs = vec![];
        slot_meta_cache = HashMap::default();
        blockstore.add_tree(tr(2) / (tr(6) / tr(7)), true, false, 2, Hash::default());
        ignore_set.insert(2);
        get_best_repair_shreds(
            &heaviest_subtree_fork_choice,
            &blockstore,
            &mut slot_meta_cache,
            &mut repairs,
            std::usize::MAX,
            &ignore_set,
        );
        assert_eq!(
            repairs,
            [0, 4, 6, 7, 5]
                .iter()
                .map(|slot| ShredRepairType::HighestShred(*slot, last_shred))
                .collect::<Vec<_>>()
        );

        // Adding unexplored child 6 to ignore set should remove it and it's
        // child 7 from the repair set
        repairs = vec![];
        ignore_set.insert(6);
        slot_meta_cache = HashMap::default();
        get_best_repair_shreds(
            &heaviest_subtree_fork_choice,
            &blockstore,
            &mut slot_meta_cache,
            &mut repairs,
            std::usize::MAX,
            &ignore_set,
        );
        assert_eq!(
            repairs,
            [0, 4, 5]
                .iter()
                .map(|slot| ShredRepairType::HighestShred(*slot, last_shred))
                .collect::<Vec<_>>()
        );
    }

    fn setup_forks() -> (Blockstore, HeaviestSubtreeForkChoice) {
        /*
            Build fork structure:
                 slot 0
                   |
                 slot 1
                 /    \
            slot 2    |
               |    slot 3
            slot 4    |
                    slot 5
        */

        let forks = tr(0) / (tr(1) / (tr(2) / (tr(4))) / (tr(3) / (tr(5))));
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&ledger_path).unwrap();
        blockstore.add_tree(forks.clone(), false, false, 2, Hash::default());

        (blockstore, HeaviestSubtreeForkChoice::new_from_tree(forks))
    }
}

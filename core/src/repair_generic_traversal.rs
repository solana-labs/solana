use {
    crate::{
        heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice, repair_service::RepairService,
        serve_repair::ShredRepairType, tree_diff::TreeDiff,
    },
    solana_ledger::{blockstore::Blockstore, blockstore_meta::SlotMeta},
    solana_sdk::{clock::Slot, hash::Hash},
    std::collections::{HashMap, HashSet},
};

struct GenericTraversal<'a> {
    tree: &'a HeaviestSubtreeForkChoice,
    pending: Vec<Slot>,
}

impl<'a> GenericTraversal<'a> {
    pub fn new(tree: &'a HeaviestSubtreeForkChoice) -> Self {
        Self {
            tree,
            pending: vec![tree.tree_root().0],
        }
    }
}

impl<'a> Iterator for GenericTraversal<'a> {
    type Item = Slot;
    fn next(&mut self) -> Option<Self::Item> {
        let next = self.pending.pop();
        if let Some(slot) = next {
            let children: Vec<_> = self
                .tree
                .children(&(slot, Hash::default()))
                .unwrap()
                .map(|(child_slot, _)| *child_slot)
                .collect();
            self.pending.extend(children);
        }
        next
    }
}

/// Does a generic traversal and inserts all slots that have a missing last index prioritized by how
/// many shreds have been received
pub fn get_unknown_last_index(
    tree: &HeaviestSubtreeForkChoice,
    blockstore: &Blockstore,
    slot_meta_cache: &mut HashMap<Slot, Option<SlotMeta>>,
    processed_slots: &mut HashSet<Slot>,
    limit: usize,
) -> Vec<ShredRepairType> {
    let iter = GenericTraversal::new(tree);
    let mut unknown_last = Vec::new();
    for slot in iter {
        if processed_slots.contains(&slot) {
            continue;
        }
        let slot_meta = slot_meta_cache
            .entry(slot)
            .or_insert_with(|| blockstore.meta(slot).unwrap());
        if let Some(slot_meta) = slot_meta {
            if slot_meta.last_index.is_none() {
                let shred_index = blockstore.get_index(slot).unwrap();
                let num_processed_shreds = if let Some(shred_index) = shred_index {
                    shred_index.data().num_shreds() as u64
                } else {
                    slot_meta.consumed
                };
                unknown_last.push((slot, slot_meta.received, num_processed_shreds));
                processed_slots.insert(slot);
            }
        }
    }
    // prioritize slots with more received shreds
    unknown_last.sort_by(|(_, _, count1), (_, _, count2)| count2.cmp(count1));
    unknown_last
        .iter()
        .take(limit)
        .map(|(slot, received, _)| ShredRepairType::HighestShred(*slot, *received))
        .collect()
}

/// Path of broken parents from start_slot to earliest ancestor not yet seen
/// Uses blockstore for fork information
fn get_unrepaired_path(
    start_slot: Slot,
    blockstore: &Blockstore,
    slot_meta_cache: &mut HashMap<Slot, Option<SlotMeta>>,
    visited: &mut HashSet<Slot>,
) -> Vec<Slot> {
    let mut path = Vec::new();
    let mut slot = start_slot;
    while visited.insert(slot) {
        let slot_meta = slot_meta_cache
            .entry(slot)
            .or_insert_with(|| blockstore.meta(slot).unwrap());
        if let Some(slot_meta) = slot_meta {
            if !slot_meta.is_full() {
                path.push(slot);
                if let Some(parent_slot) = slot_meta.parent_slot {
                    slot = parent_slot
                }
            }
        }
    }
    path.reverse();
    path
}

/// Finds repairs for slots that are closest to completion (# of missing shreds).
/// Additionaly we repair up to their oldest full ancestor (using blockstore fork info).
pub fn get_closest_completion(
    tree: &HeaviestSubtreeForkChoice,
    blockstore: &Blockstore,
    slot_meta_cache: &mut HashMap<Slot, Option<SlotMeta>>,
    processed_slots: &mut HashSet<Slot>,
    limit: usize,
) -> Vec<ShredRepairType> {
    let mut v: Vec<(Slot, u64)> = Vec::default();
    let iter = GenericTraversal::new(tree);
    for slot in iter {
        if processed_slots.contains(&slot) {
            continue;
        }
        let slot_meta = slot_meta_cache
            .entry(slot)
            .or_insert_with(|| blockstore.meta(slot).unwrap());
        if let Some(slot_meta) = slot_meta {
            if slot_meta.is_full() {
                continue;
            }
            if let Some(last_index) = slot_meta.last_index {
                let shred_index = blockstore.get_index(slot).unwrap();
                let dist = if let Some(shred_index) = shred_index {
                    let shred_count = shred_index.data().num_shreds() as u64;
                    if last_index.saturating_add(1) < shred_count {
                        datapoint_error!(
                            "repair_generic_traversal_error",
                            (
                                "error",
                                format!(
                                    "last_index + 1 < shred_count. last_index={last_index} shred_count={shred_count}",
                                ),
                                String
                            ),
                        );
                    }
                    last_index.saturating_add(1).saturating_sub(shred_count)
                } else {
                    if last_index < slot_meta.consumed {
                        datapoint_error!(
                            "repair_generic_traversal_error",
                            (
                                "error",
                                format!(
                                    "last_index < slot_meta.consumed. last_index={} slot_meta.consumed={}",
                                    last_index,
                                    slot_meta.consumed,
                                ),
                                String
                            ),
                        );
                    }
                    last_index.saturating_sub(slot_meta.consumed)
                };
                v.push((slot, dist));
                processed_slots.insert(slot);
            }
        }
    }
    v.sort_by(|(_, d1), (_, d2)| d1.cmp(d2));

    let mut visited = HashSet::new();
    let mut repairs = Vec::new();
    for (slot, _) in v {
        if repairs.len() >= limit {
            break;
        }
        // attempt to repair heaviest slots starting with their parents
        let path = get_unrepaired_path(slot, blockstore, slot_meta_cache, &mut visited);
        for slot in path {
            if repairs.len() >= limit {
                break;
            }
            let slot_meta = slot_meta_cache.get(&slot).unwrap().as_ref().unwrap();
            let new_repairs = RepairService::generate_repairs_for_slot(
                blockstore,
                slot,
                slot_meta,
                limit - repairs.len(),
            );
            repairs.extend(new_repairs);
        }
    }

    repairs
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        solana_ledger::{
            blockstore::{Blockstore, MAX_TURBINE_PROPAGATION_IN_MS},
            get_tmp_ledger_path,
        },
        solana_sdk::hash::Hash,
        std::{thread::sleep, time::Duration},
        trees::{tr, Tree, TreeWalk},
    };

    #[test]
    fn test_get_unknown_last_index() {
        let (blockstore, heaviest_subtree_fork_choice) = setup_forks();
        let last_shred = blockstore.meta(0).unwrap().unwrap().received;
        let mut slot_meta_cache = HashMap::default();
        let mut processed_slots = HashSet::default();
        let repairs = get_unknown_last_index(
            &heaviest_subtree_fork_choice,
            &blockstore,
            &mut slot_meta_cache,
            &mut processed_slots,
            10,
        );
        assert_eq!(
            repairs,
            [0, 1, 3, 5, 2, 4]
                .iter()
                .map(|slot| ShredRepairType::HighestShred(*slot, last_shred))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_get_closest_completion() {
        let (blockstore, heaviest_subtree_fork_choice) = setup_forks();
        let mut slot_meta_cache = HashMap::default();
        let mut processed_slots = HashSet::default();
        let repairs = get_closest_completion(
            &heaviest_subtree_fork_choice,
            &blockstore,
            &mut slot_meta_cache,
            &mut processed_slots,
            10,
        );
        assert_eq!(repairs, []);

        let forks = tr(0) / (tr(1) / (tr(2) / (tr(4))) / (tr(3) / (tr(5))));
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&ledger_path).unwrap();
        add_tree_with_missing_shreds(
            &blockstore,
            forks.clone(),
            false,
            true,
            100,
            Hash::default(),
        );
        let heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new_from_tree(forks);
        sleep(Duration::from_millis(MAX_TURBINE_PROPAGATION_IN_MS));
        let mut slot_meta_cache = HashMap::default();
        let mut processed_slots = HashSet::default();
        let repairs = get_closest_completion(
            &heaviest_subtree_fork_choice,
            &blockstore,
            &mut slot_meta_cache,
            &mut processed_slots,
            2,
        );
        assert_eq!(
            repairs,
            [ShredRepairType::Shred(0, 3), ShredRepairType::Shred(1, 3)]
        );
    }

    fn add_tree_with_missing_shreds(
        blockstore: &Blockstore,
        forks: Tree<Slot>,
        is_orphan: bool,
        is_slot_complete: bool,
        num_ticks: u64,
        starting_hash: Hash,
    ) {
        let mut walk = TreeWalk::from(forks);
        let mut blockhashes = HashMap::new();
        while let Some(visit) = walk.get() {
            let slot = *visit.node().data();
            if blockstore.meta(slot).unwrap().is_some()
                && blockstore.orphan(slot).unwrap().is_none()
            {
                // If slot exists in blockstore and is not an orphan, then skip it
                walk.forward();
                continue;
            }
            let parent = walk.get_parent().map(|n| *n.data());
            if parent.is_some() || !is_orphan {
                let parent_hash = parent
                    // parent won't exist for first node in a tree where
                    // `is_orphan == true`
                    .and_then(|parent| blockhashes.get(&parent))
                    .unwrap_or(&starting_hash);
                let entries = solana_entry::entry::create_ticks(
                    num_ticks * (std::cmp::max(1, slot - parent.unwrap_or(slot))),
                    0,
                    *parent_hash,
                );
                blockhashes.insert(slot, entries.last().unwrap().hash);

                let mut shreds = solana_ledger::blockstore::entries_to_test_shreds(
                    &entries,
                    slot,
                    parent.unwrap_or(slot),
                    is_slot_complete,
                    0,
                    true, // merkle_variant
                );

                // remove next to last shred
                let shred = shreds.pop().unwrap();
                shreds.pop().unwrap();
                shreds.push(shred);

                blockstore.insert_shreds(shreds, None, false).unwrap();
            }
            walk.forward();
        }
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

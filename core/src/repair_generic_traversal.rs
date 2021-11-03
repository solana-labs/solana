use crate::{
    heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice, repair_service::RepairService,
    serve_repair::ShredRepairType, tree_diff::TreeDiff,
};
use solana_ledger::{blockstore::Blockstore, blockstore_meta::SlotMeta};
use solana_sdk::{clock::Slot, hash::Hash};
use std::collections::{HashMap, HashSet};

struct GenericTraversal<'a> {
    tree: &'a HeaviestSubtreeForkChoice,
    pending: Vec<Slot>,
}

impl<'a> GenericTraversal<'a> {
    pub fn new(tree: &'a HeaviestSubtreeForkChoice) -> Self {
        Self {
            tree,
            pending: vec![tree.root().0],
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
                .iter()
                .map(|(child_slot, _)| *child_slot)
                .collect();
            self.pending.extend(children);
        }
        next
    }
}

pub fn get_unknown_last_index(
    tree: &HeaviestSubtreeForkChoice,
    blockstore: &Blockstore,
    slot_meta_cache: &mut HashMap<Slot, Option<SlotMeta>>,
    processed_slots: &mut HashSet<Slot>,
    limit: usize,
) -> Vec<ShredRepairType> {
    let mut repairs = Vec::new();
    let iter = GenericTraversal::new(tree);
    for slot in iter {
        if repairs.len() >= limit {
            break;
        }
        if processed_slots.contains(&slot) {
            continue;
        }
        let slot_meta = slot_meta_cache
            .entry(slot)
            .or_insert_with(|| blockstore.meta(slot).unwrap());
        if let Some(slot_meta) = slot_meta {
            if slot_meta.known_last_index().is_none() {
                repairs.push(ShredRepairType::HighestShred(slot, slot_meta.received));
                processed_slots.insert(slot);
            }
        }
    }
    repairs
}

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
            if let Some(last_index) = slot_meta.known_last_index() {
                let dist = last_index - slot_meta.consumed;
                v.push((slot, dist));
                processed_slots.insert(slot);
            }
        }
    }
    v.sort_by(|(_, d1), (_, d2)| d1.cmp(d2));

    let mut repairs = Vec::default();
    for (slot, _) in v {
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

    repairs
}

#[cfg(test)]
pub mod test {
    use super::*;
    use solana_ledger::get_tmp_ledger_path;
    use solana_sdk::hash::Hash;
    use trees::tr;

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

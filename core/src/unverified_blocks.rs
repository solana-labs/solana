use crate::{
    repair_weighted_traversal::{RepairWeightTraversal, Visit},
    tree_diff::TreeDiff,
};
use solana_ledger::entry::Entry;
use solana_sdk::{clock::Slot, hash::Hash};
use std::collections::HashMap;

#[derive(Default)]
pub struct UnverifiedBlockInfo {
    parent: Option<Slot>,
    children: Vec<Slot>,
    pub entries: Option<Vec<Entry>>,
    pub parent_hash: Hash,
}

#[derive(Default)]
pub struct UnverifiedBlocks {
    root: Slot,
    // Map of pending work
    // 1) Processes from smallest to largest ancestor for each fork
    // 2) Pops off earlier ancestors as they're verified and notifies
    // replay stage over channel
    unverified_blocks: HashMap<Slot, UnverifiedBlockInfo>,
}

impl UnverifiedBlocks {
    pub fn new(root: Slot) -> Self {
        let unverified_blocks: HashMap<_, _> = vec![(root, UnverifiedBlockInfo::default())]
            .into_iter()
            .collect();

        UnverifiedBlocks {
            root,
            unverified_blocks,
        }
    }

    pub fn add_unverified_block(
        &mut self,
        slot: Slot,
        parent: Slot,
        entries: Vec<Entry>,
        parent_hash: Hash,
    ) {
        info!("unverified_blocks: add_unverified_block {}", slot);
        self.unverified_blocks.insert(
            slot,
            UnverifiedBlockInfo {
                parent: Some(parent),
                entries: if entries.is_empty() {
                    None
                } else {
                    Some(entries)
                },
                children: vec![],
                parent_hash,
            },
        );

        self.unverified_blocks
            .get_mut(&parent)
            .expect("Parent must have been added before child")
            .children
            .push(slot);
    }

    pub fn get_heaviest_block(
        &mut self,
        weighted_traversal: RepairWeightTraversal,
    ) -> Option<(Slot, Hash, Vec<Entry>)> {
        for next in weighted_traversal {
            if let Visit::Unvisited(slot) = next {
                if let Some(unverified_block) = self.unverified_blocks.get_mut(&slot) {
                    if let Some(unverified_entries) = unverified_block.entries.take() {
                        return Some((slot, unverified_block.parent_hash, unverified_entries));
                    }
                }
            }
        }

        None
    }

    pub fn set_root(&mut self, new_root: Slot) {
        assert!(new_root >= self.root);
        if self.root == new_root {
            return;
        }
        let purge_slots = self.subtree_diff(self.root, new_root);
        for slot in purge_slots {
            self.unverified_blocks
                .remove(&slot)
                .expect("Slots reachable from old root must exist in tree");
        }

        info!("unverified_blocks: set_root {}", new_root);
        self.unverified_blocks
            .get_mut(&new_root)
            .expect("new root must exist in `unverified_blocks` map")
            .parent = None;
        self.root = new_root;
    }

    // Mark everything descended from the given `slot` as dead
    pub fn mark_branch_dead(&mut self, slot: Slot) -> Vec<Slot> {
        let slots_to_remove = self.subtree_diff(slot, 0);
        for slot in self.subtree_diff(slot, 0) {
            let dead_block = self
                .all_blocks
                .get_mut(&slot)
                .expect("Found slot in `self.subtree_diff()` so must exist");
            // Throw out the no longer needed entries
            dead_block.entries.take();
            // `slot` must exist in the `valid_unverified` set because only one ancestor of
            // `slot` should be marked dead through this function
            assert!(self.valid_unverified.remove(&slot))
        }
        slots_to_remove
    }

    pub fn is_dead(&self, slot: Slot) -> bool {
        !self.valid_unverified.contains(&slot) && slot != self.root
    }
}

impl TreeDiff for UnverifiedBlocks {
    fn contains_slot(&self, slot: Slot) -> bool {
        self.all_blocks.contains_key(&slot)
    }
    fn children(&self, slot: Slot) -> Option<&[Slot]> {
        self.all_blocks.get(&slot).map(|b| &b.children[..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice;
    use solana_ledger::entry::{self, Entry};
    use solana_sdk::hash::hash;
    use trees::tr;

    #[test]
    fn test_add_unverified_block() {
        // Initialize, check the root exists
        let mut unverified_blocks = UnverifiedBlocks::new(8);
        assert!(unverified_blocks.unverified_blocks.get(&8).is_some());

        let slot = 9;
        let parent = 8;
        let entries = entry::create_ticks(4, 5, Hash::default());
        let parent_hash = hash(&[8]);

        // Add a slot
        unverified_blocks.add_unverified_block(slot, parent, entries.clone(), parent_hash);

        // Add a child
        let child_slot = 10;
        let child_parent = slot;
        let child_entries = entry::create_ticks(4, 5, hash(&[9]));
        let child_parent_hash = hash(&[9]);
        unverified_blocks.add_unverified_block(
            child_slot,
            child_parent,
            child_entries.clone(),
            child_parent_hash,
        );

        // Verify slot 9
        let block_info = unverified_blocks.unverified_blocks.get(&slot).unwrap();
        assert_eq!(block_info.parent, Some(parent));
        assert_eq!(block_info.children, vec![child_slot]);
        assert_eq!(block_info.parent_hash, parent_hash);
        assert_eq!(block_info.entries, Some(entries));

        // Verify slot 10
        let block_info = unverified_blocks
            .unverified_blocks
            .get(&child_slot)
            .unwrap();
        assert_eq!(block_info.parent, Some(child_parent));
        assert!(block_info.children.is_empty());
        assert_eq!(block_info.parent_hash, child_parent_hash);
        assert_eq!(block_info.entries, Some(child_entries));
    }

    #[test]
    fn test_get_heaviest_block() {
        /*
            Build fork structure:
                 slot 0
                 /    \
            slot 1    slot 2
                        \
                        slot 3
        */
        let forks = tr(0) / (tr(1)) / (tr(2) / tr(3));
        let heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new_from_tree(forks);
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 1);
        let weighted_traversal = RepairWeightTraversal::new(&heaviest_subtree_fork_choice);
        let mut unverified_blocks = UnverifiedBlocks::new(0);

        // No non-empty block of entries has been added yet, should return nothing
        assert!(unverified_blocks
            .get_heaviest_block(weighted_traversal.clone())
            .is_none());

        // Add entries for a slot that's not the best slot. Since the best slot is
        // missing, should return that one
        let hash2 = hash(&[2]);
        unverified_blocks.add_unverified_block(2, 0, vec![Entry::default()], hash2);
        assert_eq!(
            unverified_blocks
                .get_heaviest_block(weighted_traversal.clone())
                .unwrap(),
            (2, hash2, vec![Entry::default()])
        );

        // Calling again should return nothing, since we've already verified everything
        assert!(unverified_blocks
            .get_heaviest_block(weighted_traversal.clone())
            .is_none());

        // Add entries for slot 1 and 3, should prefer 1 then 3.
        let hash1 = hash(&[1]);
        unverified_blocks.add_unverified_block(1, 0, vec![Entry::default()], hash1);
        let hash3 = hash(&[3]);
        unverified_blocks.add_unverified_block(3, 2, vec![Entry::default()], hash3);
        assert_eq!(
            unverified_blocks
                .get_heaviest_block(weighted_traversal.clone())
                .unwrap(),
            (1, hash1, vec![Entry::default()])
        );
        assert_eq!(
            unverified_blocks
                .get_heaviest_block(weighted_traversal.clone())
                .unwrap(),
            (3, hash3, vec![Entry::default()])
        );
        assert!(unverified_blocks
            .get_heaviest_block(weighted_traversal.clone())
            .is_none());
    }

    #[test]
    fn test_set_root() {
        /*
            Build fork structure:
                 slot 0
                 /    \
            slot 1    slot 2
                        \
                        slot 3
        */
        let mut unverified_blocks = UnverifiedBlocks::new(0);
        unverified_blocks.add_unverified_block(1, 0, vec![], Hash::default());
        unverified_blocks.add_unverified_block(2, 0, vec![], Hash::default());
        unverified_blocks.add_unverified_block(3, 2, vec![], Hash::default());

        // Set existing root, nothing should change
        unverified_blocks.set_root(0);
        assert_eq!(unverified_blocks.root, 0);
        for i in 1..=3 {
            assert!(unverified_blocks.unverified_blocks.contains_key(&i));
        }

        // Set a root to 2, slot 1 should be purged
        unverified_blocks.set_root(2);
        assert!(!unverified_blocks.unverified_blocks.contains_key(&1));
        for i in 2..=3 {
            assert!(unverified_blocks.unverified_blocks.contains_key(&i));
        }

        // Set a root to 3, slot 2 should be purged
        unverified_blocks.set_root(3);
        assert!(!unverified_blocks.unverified_blocks.contains_key(&2));
        assert!(unverified_blocks.unverified_blocks.contains_key(&3));
    }
}

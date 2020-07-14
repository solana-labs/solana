use solana_sdk::clock::Slot;
use std::collections::HashSet;

pub trait TreeDiff {
    fn children(&self, slot: Slot) -> Option<&[Slot]>;

    fn contains_slot(&self, slot: Slot) -> bool;

    // Find all nodes reachable from `root1`, excluding subtree at `root2`
    fn subtree_diff(&self, root1: Slot, root2: Slot) -> HashSet<Slot> {
        if !self.contains_slot(root1) {
            return HashSet::new();
        }
        let mut pending_slots = vec![root1];
        let mut reachable_set = HashSet::new();
        while !pending_slots.is_empty() {
            let current_slot = pending_slots.pop().unwrap();
            if current_slot == root2 {
                continue;
            }
            reachable_set.insert(current_slot);
            for child in self
                .children(current_slot)
                .expect("slot was discovered earlier, must exist")
            {
                pending_slots.push(*child);
            }
        }

        reachable_set
    }
}

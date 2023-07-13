use std::{collections::HashSet, hash::Hash};

pub trait TreeDiff<'a> {
    type TreeKey: 'a + Hash + PartialEq + Eq + Copy;
    type ChildIter: Iterator<Item = &'a Self::TreeKey>;

    fn children(&self, key: &Self::TreeKey) -> Option<Self::ChildIter>;

    fn contains_slot(&self, slot: &Self::TreeKey) -> bool;

    // Find all nodes reachable from `root1`, excluding subtree at `root2`
    fn subtree_diff(&self, root1: Self::TreeKey, root2: Self::TreeKey) -> HashSet<Self::TreeKey> {
        if !self.contains_slot(&root1) {
            return HashSet::new();
        }
        let mut pending_keys = vec![root1];
        let mut reachable_set = HashSet::new();
        while let Some(current_key) = pending_keys.pop() {
            if current_key == root2 {
                continue;
            }
            for child in self
                .children(&current_key)
                .expect("slot was discovered earlier, must exist")
            {
                pending_keys.push(*child);
            }
            reachable_set.insert(current_key);
        }

        reachable_set
    }
}

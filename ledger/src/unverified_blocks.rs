use crate::{bank_forks::BankForks, entry::Entry};
use solana_sdk::clock::Slot;
use solana_sdk::hash::Hash;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::RwLock;

pub struct UnverifiedBlockInfo {
    parent: Slot,
    // used as a correctness check
    children: Vec<Slot>,
    fork_weight: u128,
    pub entries: Vec<Entry>,
    pub parent_hash: Hash,
}

#[derive(Default)]
pub struct UnverifiedBlocks {
    root: Slot,
    // Map of pending work
    // 1) Processes from smallest to largest ancestor for each fork
    // 2) Pops off earlier ancestors as they're verified and notifies
    // replay stage over channel
    pub unverified_blocks: HashMap<Slot, UnverifiedBlockInfo>,

    // Map from fork weight to slot
    fork_weights: BTreeMap<u128, BTreeSet<Slot>>,
}

impl UnverifiedBlocks {
    pub fn add_unverified_block(
        &mut self,
        slot: Slot,
        parent: Slot,
        entries: Vec<Entry>,
        fork_weight: u128,
        parent_hash: Hash,
    ) {
        self.unverified_blocks.insert(
            slot,
            UnverifiedBlockInfo {
                parent,
                entries,
                children: vec![],
                parent_hash,
                fork_weight,
            },
        );
        let fork_weights = self
            .fork_weights
            .entry(fork_weight)
            .or_insert_with(BTreeSet::new);
        fork_weights.insert(slot);

        if let Some(parent) = self.unverified_blocks.get_mut(&parent) {
            parent.children.push(slot);
        }
    }

    pub fn get_unverified_ancestors(&self, slot: Slot) -> VecDeque<Slot> {
        let mut current_slot = slot;
        let mut current_block = self.unverified_blocks.get(&slot);
        let mut unverified_ancestors = VecDeque::new();
        assert!(current_block.is_some());
        while let Some(block) = current_block {
            unverified_ancestors.push_front(current_slot);
            current_slot = block.parent;
            current_block = self.unverified_blocks.get(&current_slot);
        }
        unverified_ancestors
    }

    // Returns the heaviest leaf of any fork. Need to verify
    // from least to greatest all its ancestors which can be
    // retrieved through Self::get_unverified_ancestors().
    pub fn next_heaviest_leaf(&self) -> Option<Slot> {
        // Take heaviest bank, greatest slot, should be a leaf.
        self.fork_weights
            .values()
            .rev()
            .next()
            .map(|slots| {
                slots
                    .iter()
                    .find(|slot| {
                        self.unverified_blocks
                            .get(&slot)
                            .expect(
                                "If exists in fork_weights, must
            exist in unverified blocks as both are in sync",
                            )
                            .children
                            .is_empty()
                    })
                    .expect("at least one heaviest fork must be a leaf")
            })
            .cloned()
    }

    pub fn set_root(&mut self, bank_forks: &RwLock<BankForks>) {
        let root = bank_forks.read().unwrap().root();
        if root == self.root {
            return;
        }
        self.root = root;

        let mut slot_weights_to_remove = Vec::new();
        self.unverified_blocks.retain(|slot, block_info| {
            let keep = bank_forks.read().unwrap().get(*slot).is_some();
            if !keep {
                slot_weights_to_remove.push((block_info.fork_weight, *slot));
            }
            keep
        });

        let mut weights_to_remove = Vec::new();
        for (weight, slot) in slot_weights_to_remove {
            let slots_with_weight = self
                .fork_weights
                .get_mut(&weight)
                .expect("Weight must exist because a slot with this weight was just removed above");
            // Slot must exist because a slot with this weight was just removed above
            assert!(slots_with_weight.remove(&slot));
            if slots_with_weight.is_empty() {
                weights_to_remove.push(weight);
            }
        }

        for weight in weights_to_remove {
            self.fork_weights.remove(&weight);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entry,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
    };
    use solana_runtime::bank::Bank;
    use solana_sdk::{hash::hash, pubkey::Pubkey};

    #[test]
    fn test_add_unverified_block() {
        let mut unverified_blocks = UnverifiedBlocks::default();
        let slot = 9;
        let parent = 8;
        let entries = entry::create_ticks(4, 5, Hash::default());
        let parent_hash = hash(&[8]);
        let fork_weight = 100;

        // Add a slot
        unverified_blocks.add_unverified_block(
            slot,
            parent,
            entries.clone(),
            fork_weight,
            parent_hash,
        );

        // Add a child
        let child_slot = 10;
        let child_parent = slot;
        let child_entries = entry::create_ticks(4, 5, hash(&[9]));
        let child_fork_weight = 200;
        let child_parent_hash = hash(&[9]);
        unverified_blocks.add_unverified_block(
            child_slot,
            child_parent,
            child_entries.clone(),
            child_fork_weight,
            child_parent_hash,
        );

        // Verify slot 9
        let block_info = unverified_blocks.unverified_blocks.get(&slot).unwrap();
        assert_eq!(block_info.parent, parent);
        assert_eq!(block_info.children, vec![child_slot]);
        assert_eq!(block_info.parent_hash, parent_hash);
        assert_eq!(block_info.entries, entries);
        assert_eq!(block_info.fork_weight, fork_weight);
        assert!(unverified_blocks
            .fork_weights
            .get(&fork_weight)
            .unwrap()
            .contains(&slot));

        // Verify slot 10
        let block_info = unverified_blocks
            .unverified_blocks
            .get(&child_slot)
            .unwrap();
        assert_eq!(block_info.parent, child_parent);
        assert!(block_info.children.is_empty());
        assert_eq!(block_info.parent_hash, child_parent_hash);
        assert_eq!(block_info.entries, child_entries);
        assert_eq!(block_info.fork_weight, child_fork_weight);
        assert!(unverified_blocks
            .fork_weights
            .get(&child_fork_weight)
            .unwrap()
            .contains(&child_slot));
    }

    #[test]
    fn test_get_unverified_ancestors() {
        let mut unverified_blocks = UnverifiedBlocks::default();
        unverified_blocks.add_unverified_block(1, 0, vec![], 100, Hash::default());
        unverified_blocks.add_unverified_block(2, 1, vec![], 100, Hash::default());
        unverified_blocks.add_unverified_block(3, 1, vec![], 100, Hash::default());
        assert_eq!(
            unverified_blocks
                .get_unverified_ancestors(1)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(
            unverified_blocks
                .get_unverified_ancestors(2)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            unverified_blocks
                .get_unverified_ancestors(3)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
    }

    #[test]
    fn test_next_heaviest_leaf() {
        let mut unverified_blocks = UnverifiedBlocks::default();
        unverified_blocks.add_unverified_block(1, 0, vec![], 100, Hash::default());
        unverified_blocks.add_unverified_block(2, 0, vec![], 100, Hash::default());

        // When weights are equal, should pick the smaller slot
        assert_eq!(unverified_blocks.next_heaviest_leaf().unwrap(), 1);

        // When weights are equal, but one is not a leaf, should pick the smallest leaf
        unverified_blocks.add_unverified_block(4, 1, vec![], 100, Hash::default());
        assert_eq!(unverified_blocks.next_heaviest_leaf().unwrap(), 2);

        unverified_blocks.add_unverified_block(3, 2, vec![], 200, Hash::default());
        assert_eq!(unverified_blocks.next_heaviest_leaf().unwrap(), 3);
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
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new(&genesis_config);
        let mut bank_forks = BankForks::new(0, bank);
        let bank0 = bank_forks[0].clone();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.insert(bank1);
        let bank2 = Bank::new_from_parent(&bank0, &Pubkey::default(), 2);
        bank_forks.insert(bank2);
        let bank2 = bank_forks[2].clone();
        let bank3 = Bank::new_from_parent(&bank2, &Pubkey::default(), 3);
        bank_forks.insert(bank3);

        let bank_forks = RwLock::new(bank_forks);
        let mut unverified_blocks = UnverifiedBlocks::default();
        unverified_blocks.add_unverified_block(1, 0, vec![], 100, Hash::default());
        unverified_blocks.add_unverified_block(2, 0, vec![], 100, Hash::default());
        unverified_blocks.add_unverified_block(3, 2, vec![], 200, Hash::default());

        unverified_blocks.set_root(&bank_forks);
        // Nothing should be removed because all slots are present in BankForks
        assert_eq!(unverified_blocks.root, 0);
        for i in 1..=3 {
            assert!(unverified_blocks.unverified_blocks.contains_key(&i));
        }
        assert_eq!(
            unverified_blocks
                .fork_weights
                .get(&100)
                .unwrap()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            unverified_blocks
                .fork_weights
                .get(&200)
                .unwrap()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![3]
        );

        // Set a root to 2
        bank_forks.write().unwrap().set_root(2, &None, None);
        unverified_blocks.set_root(&bank_forks);
        // slot 1 should be purged
        assert!(!unverified_blocks.unverified_blocks.contains_key(&1));
        for i in 2..=3 {
            assert!(unverified_blocks.unverified_blocks.contains_key(&i));
        }
        assert_eq!(
            unverified_blocks
                .fork_weights
                .get(&100)
                .unwrap()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(
            unverified_blocks
                .fork_weights
                .get(&200)
                .unwrap()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![3]
        );

        // Set a root to 3
        bank_forks.write().unwrap().set_root(3, &None, None);
        unverified_blocks.set_root(&bank_forks);
        // slot 2 should be purged
        assert!(!unverified_blocks.unverified_blocks.contains_key(&2));
        assert!(unverified_blocks.unverified_blocks.contains_key(&3));
        assert!(!unverified_blocks.fork_weights.contains_key(&100));
        assert_eq!(
            unverified_blocks
                .fork_weights
                .get(&200)
                .unwrap()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![3]
        );
    }
}

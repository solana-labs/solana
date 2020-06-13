use crate::entry::Entry;
use solana_sdk::clock::Slot;
use solana_sdk::hash::Hash;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

pub struct UnverifiedBlockInfo {
    parent: Slot,
    // used as a correctness check
    children: Vec<Slot>,
    pub entries: Vec<Entry>,
    pub parent_hash: Hash,
}

#[derive(Default)]
pub struct UnverifiedBlocks {
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
        // TODO: update weights
        self.unverified_blocks.insert(
            slot,
            UnverifiedBlockInfo {
                parent,
                entries,
                children: vec![],
                parent_hash,
            },
        );
        let fork_weights = self
            .fork_weights
            .entry(fork_weight)
            .or_insert_with(BTreeSet::new());
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
        let res = self
            .fork_weights
            .values()
            .rev()
            .next()
            .map(|slots| slots.iter().rev().next())
            .unwrap_or(None)
            .cloned();

        if let Some(leaf) = res {
            let leaf_block = self.unverified_blocks.get(&leaf).expect(
                "If exists in fork_weights, must
        exist in unverified blocks as both are in sync",
            );
            {
                assert!(leaf_block.children.is_empty());
            }
        }

        res
    }

    pub fn set_root(&mut self, root: Slot) {
        let mut slots_to_remove = Vec::new();
        for (slot, _info) in self.unverified_blocks.iter() {
            if *slot <= root {
                slots_to_remove.push(*slot);
            }
        }
        for slot in &slots_to_remove {
            self.unverified_blocks.remove(slot);
        }
        let mut weights_to_remove = Vec::new();
        for (weight, slots) in &mut self.fork_weights {
            for slot in &slots_to_remove {
                slots.remove(slot);
            }
            if slots.is_empty() {
                weights_to_remove.push(*weight);
            }
        }
        for weight in weights_to_remove {
            self.fork_weights.remove(&weight);
        }
    }
}

use solana_ledger::entry::Entry;
use solana_sdk::clock::Slot;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

struct UnverifiedBlockInfo {
    parent: Slot,
    // used as a correctness check
    children: Vec<Slot>,
    entries: Vec<Entry>,
    fork_weight: u128,
}

#[derive(Default)]
pub(crate) struct UnverifiedBlocks {
    // Map of pending work
    // 1) Processes from smallest to largest ancestor for each fork
    // 2) Pops off earlier ancestors as they're verified and notifies
    // replay stage over channel
    unverified_blocks: HashMap<Slot, UnverifiedBlockInfo>,

    // Map from fork weight to slot
    fork_weights: BTreeMap<u128, BTreeSet<Slot>>,

    verification_results: HashMap<Slot, bool>,
}

impl UnverifiedBlocks {
    pub fn is_verified(&self, slot: Slot) -> Option<bool> {
        self.verification_results.get(&slot).cloned()
    }

    pub fn add_unverified_block(
        &mut self,
        slot: Slot,
        parent: Slot,
        entries: Vec<Entry>,
        fork_weight: u128,
    ) {
        // TODO: update weights
        self.unverified_blocks.insert(
            slot,
            UnverifiedBlockInfo {
                parent,
                entries,
                children: vec![],
                fork_weight,
            },
        );
        if let Some(parent) = self.unverified_blocks.get_mut(&parent) {
            parent.children.push(slot);
        }
    }

    fn get_unverified_ancestors(&self, slot: Slot) -> VecDeque<Slot> {
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
    fn next_heaviest_leaf(&self) -> Option<Slot> {
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
        exisst in unverified blocks as both are in sync",
            );
            {
                assert!(leaf_block.children.is_empty());
            }
        }

        res
    }

    fn set_root(&mut self, root: Slot) {
        // Purge both `self.unverified_blocks` and `self.fork_weights`
    }
}

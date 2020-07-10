use crate::repair_weighted_traversal::{RepairWeightTraversal, Visit};
use solana_ledger::entry::Entry;
use solana_runtime::bank_forks::BankForks;
use solana_sdk::{clock::Slot, hash::Hash};
use std::collections::HashMap;
use std::sync::RwLock;

pub struct UnverifiedBlockInfo {
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
    unverified_blocks: HashMap<Slot, UnverifiedBlockInfo>,
}

impl UnverifiedBlocks {
    pub fn add_unverified_block(&mut self, slot: Slot, entries: Vec<Entry>, parent_hash: Hash) {
        self.unverified_blocks.insert(
            slot,
            UnverifiedBlockInfo {
                entries,
                parent_hash,
            },
        );
    }

    pub fn get_heaviest_ancestor(
        &mut self,
        weighted_traversal: RepairWeightTraversal,
    ) -> Option<(Slot, UnverifiedBlockInfo)> {
        for next in weighted_traversal {
            if let Visit::Unvisited(slot) = next {
                if let Some(unverified_block) = self.unverified_blocks.remove(&slot) {
                    return Some((slot, unverified_block));
                }
            }
        }

        None
    }

    pub fn set_root(&mut self, bank_forks: &RwLock<BankForks>) {
        let root = bank_forks.read().unwrap().root();
        if root == self.root {
            return;
        }
        self.root = root;

        self.unverified_blocks
            .retain(|slot, _| bank_forks.read().unwrap().get(*slot).is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::test::VoteSimulator;
    use solana_ledger::entry;
    use solana_sdk::hash::hash;
    use trees::tr;

    #[test]
    fn test_add_unverified_block() {
        let mut unverified_blocks = UnverifiedBlocks::default();
        let slot = 9;
        let entries = entry::create_ticks(4, 5, Hash::default());
        let parent_hash = hash(&[8]);

        // Add a slot
        unverified_blocks.add_unverified_block(slot, entries.clone(), parent_hash);

        // Verify slot 9
        let block_info = unverified_blocks.unverified_blocks.get(&slot).unwrap();
        assert_eq!(block_info.parent_hash, parent_hash);
        assert_eq!(block_info.entries, entries);
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
        let forks = tr(0) / (tr(1)) / (tr(2) / tr(3));

        let mut vote_simulator = VoteSimulator::new(1);
        vote_simulator.fill_bank_forks(forks, &HashMap::new());
        let bank_forks = &vote_simulator.bank_forks;
        let mut unverified_blocks = UnverifiedBlocks::default();
        unverified_blocks.add_unverified_block(1, vec![], Hash::default());
        unverified_blocks.add_unverified_block(2, vec![], Hash::default());
        unverified_blocks.add_unverified_block(3, vec![], Hash::default());

        unverified_blocks.set_root(bank_forks);
        // Nothing should be removed because all slots are present in BankForks
        assert_eq!(unverified_blocks.root, 0);
        for i in 1..=3 {
            assert!(unverified_blocks.unverified_blocks.contains_key(&i));
        }

        // Set a root to 2
        bank_forks.write().unwrap().set_root(2, &None, None);
        unverified_blocks.set_root(bank_forks);
        // slot 1 should be purged
        assert!(!unverified_blocks.unverified_blocks.contains_key(&1));
        for i in 2..=3 {
            assert!(unverified_blocks.unverified_blocks.contains_key(&i));
        }

        // Set a root to 3
        bank_forks.write().unwrap().set_root(3, &None, None);
        unverified_blocks.set_root(bank_forks);
        // slot 2 should be purged
        assert!(!unverified_blocks.unverified_blocks.contains_key(&2));
        assert!(unverified_blocks.unverified_blocks.contains_key(&3));
    }
}

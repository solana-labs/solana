#[cfg(test)]
use solana_ledger::bank_forks::BankForks;
use solana_runtime::{bank::Bank, epoch_stakes::EpochStakes};
use solana_sdk::{
    clock::{Epoch, Slot},
    epoch_schedule::EpochSchedule,
    pubkey::Pubkey,
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
#[cfg(test)]
use trees::{Tree, TreeWalk};

mod fork_choice;
pub type ForkWeight = u64;

#[derive(PartialEq, Eq, Clone, Debug, PartialOrd, Ord)]
enum UpdateLabel {
    Aggregate,
    Add,
    Subtract,
}

#[derive(PartialEq, Eq, Clone, Debug)]
enum UpdateOperation {
    Aggregate,
    Add(u64),
    Subtract(u64),
}

impl UpdateOperation {
    fn update_stake(&mut self, new_stake: u64) {
        match self {
            Self::Aggregate => panic!("Should not get here"),
            Self::Add(stake) => *stake += new_stake,
            Self::Subtract(stake) => *stake += new_stake,
        }
    }
}

struct ForkInfo {
    // Amount of stake that has voted for exactly this slot
    stake_voted_at: ForkWeight,
    // Amount of stake that has voted for this slot and the subtree
    // rooted at this slot
    stake_voted_subtree: ForkWeight,
    // Best slot in the subtree rooted at this slot, does not
    // have to be a direct child in `children`
    best_slot: Slot,
    parent: Option<Slot>,
    children: Vec<Slot>,
}

#[derive(Default)]
pub struct HeaviestSubtreeForkChoice {
    fork_infos: HashMap<Slot, ForkInfo>,
    latest_votes: HashMap<Pubkey, Slot>,
    root: Slot,
}

impl HeaviestSubtreeForkChoice {
    pub(crate) fn new(root: Slot) -> Self {
        let mut heaviest_subtree_fork_choice = Self {
            root,
            ..HeaviestSubtreeForkChoice::default()
        };
        heaviest_subtree_fork_choice.add_new_leaf_slot(root, None);
        heaviest_subtree_fork_choice
    }

    // Given a root and a list of `frozen_banks` sorted smallest to greatest by slot,
    // return a new HeaviestSubtreeForkChoice
    pub(crate) fn new_from_frozen_banks(root: Slot, frozen_banks: &[Arc<Bank>]) -> Self {
        let mut heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new(root);
        let mut prev_slot = root;
        for bank in frozen_banks.iter() {
            assert!(bank.is_frozen());
            if bank.slot() > root {
                // Make sure the list is sorted
                assert!(bank.slot() > prev_slot);
                prev_slot = bank.slot();
                heaviest_subtree_fork_choice
                    .add_new_leaf_slot(bank.slot(), Some(bank.parent_slot()));
            }
        }

        heaviest_subtree_fork_choice
    }

    #[cfg(test)]
    pub(crate) fn new_from_bank_forks(bank_forks: &BankForks) -> Self {
        let mut frozen_banks: Vec<_> = bank_forks.frozen_banks().values().cloned().collect();

        frozen_banks.sort_by_key(|bank| bank.slot());
        let root = bank_forks.root();
        Self::new_from_frozen_banks(root, &frozen_banks)
    }

    #[cfg(test)]
    pub(crate) fn new_from_tree(forks: Tree<Slot>) -> Self {
        let root = forks.root().data;
        let mut walk = TreeWalk::from(forks);
        let mut heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new(root);
        while let Some(visit) = walk.get() {
            let slot = visit.node().data;
            if heaviest_subtree_fork_choice.fork_infos.contains_key(&slot) {
                walk.forward();
                continue;
            }
            let parent = walk.get_parent().map(|n| n.data);
            heaviest_subtree_fork_choice.add_new_leaf_slot(slot, parent);
            walk.forward();
        }

        heaviest_subtree_fork_choice
    }

    pub fn best_slot(&self, slot: Slot) -> Option<Slot> {
        self.fork_infos
            .get(&slot)
            .map(|fork_info| fork_info.best_slot)
    }

    pub fn best_overall_slot(&self) -> Slot {
        self.best_slot(self.root).unwrap()
    }

    pub fn stake_voted_subtree(&self, slot: Slot) -> Option<u64> {
        self.fork_infos
            .get(&slot)
            .map(|fork_info| fork_info.stake_voted_subtree)
    }

    // Add new votes, returns the best slot
    pub fn add_votes(
        &mut self,
        // newly updated votes on a fork
        pubkey_votes: &[(Pubkey, Slot)],
        epoch_stakes: &HashMap<Epoch, EpochStakes>,
        epoch_schedule: &EpochSchedule,
    ) -> Slot {
        // Generate the set of updates
        let update_operations =
            self.generate_update_operations(pubkey_votes, epoch_stakes, epoch_schedule);

        // Finalize all updates
        self.process_update_operations(update_operations);
        self.best_overall_slot()
    }

    pub fn set_root(&mut self, root: Slot) {
        self.root = root;
        let mut pending_slots = vec![root];
        let mut new_fork_infos = HashMap::new();
        while !pending_slots.is_empty() {
            let current_slot = pending_slots.pop().unwrap();
            let fork_info = self
                .fork_infos
                .remove(&current_slot)
                .expect("Anything reachable from root must exist in the map");
            for child in &fork_info.children {
                pending_slots.push(*child);
            }
            new_fork_infos.insert(current_slot, fork_info);
        }

        std::mem::swap(&mut self.fork_infos, &mut new_fork_infos);
        self.fork_infos
            .get_mut(&root)
            .expect("new root must exist in fork_infos map")
            .parent = None;
    }

    pub fn add_new_leaf_slot(&mut self, slot: Slot, parent: Option<Slot>) {
        self.fork_infos
            .entry(slot)
            .and_modify(|slot_info| slot_info.parent = parent)
            .or_insert(ForkInfo {
                stake_voted_at: 0,
                stake_voted_subtree: 0,
                // The `best_slot` of a leaf is itself
                best_slot: slot,
                children: vec![],
                parent,
            });

        if parent.is_none() {
            return;
        }

        let parent = parent.unwrap();

        // Parent must already exist by time child is being added
        self.fork_infos
            .get_mut(&parent)
            .unwrap()
            .children
            .push(slot);

        // Propagate leaf up the tree to any ancestors who considered the previous leaf
        // the `best_slot`
        self.propagate_new_leaf(slot, parent)
    }

    fn propagate_new_leaf(&mut self, slot: Slot, parent: Slot) {
        let parent_best_slot = self
            .best_slot(parent)
            .expect("parent must exist in self.fork_infos after being created above");
        let should_replace_parent_best_slot = parent_best_slot == parent
            || (parent_best_slot > slot
                // if `stake_voted_subtree(parent).unwrap() - stake_voted_at(parent) == 0`
                // then none of the children of `parent` have been voted on. Because
                // this new leaf also hasn't been voted on, it must have the same weight
                // as any existing child, so this new leaf can replace the previous best child
                // if the new leaf is a lower slot.
                && self.stake_voted_subtree(parent).unwrap() - self.stake_voted_at(parent).unwrap() == 0);

        if should_replace_parent_best_slot {
            let mut ancestor = Some(parent);
            loop {
                if ancestor.is_none() {
                    break;
                }
                let ancestor_fork_info = self.fork_infos.get_mut(&ancestor.unwrap()).unwrap();
                if ancestor_fork_info.best_slot == parent_best_slot {
                    ancestor_fork_info.best_slot = slot;
                } else {
                    break;
                }
                ancestor = ancestor_fork_info.parent;
            }
        }
    }

    #[allow(clippy::map_entry)]
    fn insert_aggregate_operations(
        &self,
        update_operations: &mut BTreeMap<(Slot, UpdateLabel), UpdateOperation>,
        slot: Slot,
    ) {
        for parent in self.ancestor_iterator(slot) {
            let label = (parent, UpdateLabel::Aggregate);
            if update_operations.contains_key(&label) {
                break;
            } else {
                update_operations.insert(label, UpdateOperation::Aggregate);
            }
        }
    }

    fn ancestor_iterator(&self, start_slot: Slot) -> AncestorIterator {
        AncestorIterator::new(start_slot, &self.fork_infos)
    }

    fn aggregate_slot(&mut self, slot: Slot) {
        let mut stake_voted_subtree;
        let mut best_slot = slot;
        if let Some(fork_info) = self.fork_infos.get(&slot) {
            stake_voted_subtree = fork_info.stake_voted_at;
            let mut best_child_stake_voted_subtree = 0;
            let mut best_child_slot = slot;
            let should_print = fork_info.children.len() > 1;
            for &child in &fork_info.children {
                let child_stake_voted_subtree = self.stake_voted_subtree(child).unwrap();
                if should_print {
                    info!(
                        "child: {} of slot: {} has weight: {}",
                        child, slot, child_stake_voted_subtree
                    );
                }
                stake_voted_subtree += child_stake_voted_subtree;
                if best_child_slot == slot ||
                child_stake_voted_subtree > best_child_stake_voted_subtree ||
            // tiebreaker by slot height, prioritize earlier slot
            (child_stake_voted_subtree == best_child_stake_voted_subtree && child < best_child_slot)
                {
                    best_child_stake_voted_subtree = child_stake_voted_subtree;
                    best_child_slot = child;
                    best_slot = self
                        .best_slot(child)
                        .expect("`child` must exist in `self.fork_infos`");
                }
            }
        } else {
            return;
        }

        let fork_info = self.fork_infos.get_mut(&slot).unwrap();
        fork_info.stake_voted_subtree = stake_voted_subtree;
        fork_info.best_slot = best_slot;
    }

    fn generate_update_operations(
        &mut self,
        pubkey_votes: &[(Pubkey, Slot)],
        epoch_stakes: &HashMap<Epoch, EpochStakes>,
        epoch_schedule: &EpochSchedule,
    ) -> BTreeMap<(Slot, UpdateLabel), UpdateOperation> {
        let mut update_operations: BTreeMap<(Slot, UpdateLabel), UpdateOperation> = BTreeMap::new();

        // Sort the `pubkey_votes` in a BTreeMap by the slot voted
        for &(pubkey, new_vote_slot) in pubkey_votes.iter() {
            let pubkey_latest_vote = self.latest_votes.get(&pubkey).unwrap_or(&0);
            // Filter out any votes or slots <= any slot this pubkey has
            // already voted for, we only care about the latest votes
            if new_vote_slot <= *pubkey_latest_vote {
                continue;
            }

            // Remove this pubkey stake from previous fork
            if let Some(old_latest_vote_slot) = self.latest_votes.insert(pubkey, new_vote_slot) {
                let epoch = epoch_schedule.get_epoch(old_latest_vote_slot);
                let stake_update = epoch_stakes
                    .get(&epoch)
                    .map(|epoch_stakes| epoch_stakes.vote_account_stake(&pubkey))
                    .unwrap_or(0);
                if stake_update > 0 {
                    update_operations
                        .entry((old_latest_vote_slot, UpdateLabel::Subtract))
                        .and_modify(|update| update.update_stake(stake_update))
                        .or_insert(UpdateOperation::Subtract(stake_update));
                    self.insert_aggregate_operations(&mut update_operations, old_latest_vote_slot);
                }
            }

            // Add this pubkey stake to new fork
            let epoch = epoch_schedule.get_epoch(new_vote_slot);
            let stake_update = epoch_stakes
                .get(&epoch)
                .map(|epoch_stakes| epoch_stakes.vote_account_stake(&pubkey))
                .unwrap_or(0);
            update_operations
                .entry((new_vote_slot, UpdateLabel::Add))
                .and_modify(|update| update.update_stake(stake_update))
                .or_insert(UpdateOperation::Add(stake_update));
            self.insert_aggregate_operations(&mut update_operations, new_vote_slot);
        }

        update_operations
    }

    fn process_update_operations(
        &mut self,
        update_operations: BTreeMap<(Slot, UpdateLabel), UpdateOperation>,
    ) {
        // Iterate through the update operations from greatest to smallest slot
        for ((slot, _), operation) in update_operations.into_iter().rev() {
            match operation {
                UpdateOperation::Aggregate => self.aggregate_slot(slot),
                UpdateOperation::Add(stake) => self.add_slot_stake(slot, stake),
                UpdateOperation::Subtract(stake) => self.subtract_slot_stake(slot, stake),
            }
        }
    }

    fn add_slot_stake(&mut self, slot: Slot, stake: u64) {
        if let Some(fork_info) = self.fork_infos.get_mut(&slot) {
            fork_info.stake_voted_at += stake;
            fork_info.stake_voted_subtree += stake;
        }
    }

    fn subtract_slot_stake(&mut self, slot: Slot, stake: u64) {
        if let Some(fork_info) = self.fork_infos.get_mut(&slot) {
            fork_info.stake_voted_at -= stake;
            fork_info.stake_voted_subtree -= stake;
        }
    }

    fn stake_voted_at(&self, slot: Slot) -> Option<u64> {
        self.fork_infos
            .get(&slot)
            .map(|fork_info| fork_info.stake_voted_at)
    }

    #[cfg(test)]
    fn set_stake_voted_at(&mut self, slot: Slot, stake_voted_at: u64) {
        self.fork_infos.get_mut(&slot).unwrap().stake_voted_at = stake_voted_at;
    }

    #[cfg(test)]
    fn is_leaf(&self, slot: Slot) -> bool {
        self.fork_infos.get(&slot).unwrap().children.is_empty()
    }

    #[cfg(test)]
    fn children(&self, slot: Slot) -> Option<&Vec<Slot>> {
        self.fork_infos
            .get(&slot)
            .map(|fork_info| &fork_info.children)
    }

    #[cfg(test)]
    fn parent(&self, slot: Slot) -> Option<Slot> {
        self.fork_infos
            .get(&slot)
            .map(|fork_info| fork_info.parent)
            .unwrap_or(None)
    }
}

struct AncestorIterator<'a> {
    current_slot: Slot,
    fork_infos: &'a HashMap<Slot, ForkInfo>,
}

impl<'a> AncestorIterator<'a> {
    fn new(start_slot: Slot, fork_infos: &'a HashMap<Slot, ForkInfo>) -> Self {
        Self {
            current_slot: start_slot,
            fork_infos,
        }
    }
}

impl<'a> Iterator for AncestorIterator<'a> {
    type Item = Slot;
    fn next(&mut self) -> Option<Self::Item> {
        let parent = self
            .fork_infos
            .get(&self.current_slot)
            .map(|fork_info| fork_info.parent)
            .unwrap_or(None);

        parent
            .map(|parent| {
                self.current_slot = parent;
                Some(self.current_slot)
            })
            .unwrap_or(None)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::consensus::test::VoteSimulator;
    use solana_runtime::{
        bank::Bank,
        genesis_utils::{self, GenesisConfigInfo, ValidatorVoteKeypairs},
    };
    use solana_sdk::signature::{Keypair, Signer};
    use std::{collections::HashSet, ops::Range};
    use trees::tr;

    #[test]
    fn test_ancestor_iterator() {
        let mut heaviest_subtree_fork_choice = setup_forks();
        let parents: Vec<_> = heaviest_subtree_fork_choice.ancestor_iterator(6).collect();
        assert_eq!(parents, vec![5, 3, 1, 0]);
        let parents: Vec<_> = heaviest_subtree_fork_choice.ancestor_iterator(4).collect();
        assert_eq!(parents, vec![2, 1, 0]);
        let parents: Vec<_> = heaviest_subtree_fork_choice.ancestor_iterator(1).collect();
        assert_eq!(parents, vec![0]);
        let parents: Vec<_> = heaviest_subtree_fork_choice.ancestor_iterator(0).collect();
        assert!(parents.is_empty());

        // Set a root, everything but slots 2, 4 should be removed
        heaviest_subtree_fork_choice.set_root(2);
        let parents: Vec<_> = heaviest_subtree_fork_choice.ancestor_iterator(4).collect();
        assert_eq!(parents, vec![2]);
    }

    #[test]
    fn test_new_from_frozen_banks() {
        /*
            Build fork structure:
                 slot 0
                   |
                 slot 1
                 /    \
            slot 2    |
               |    slot 3
            slot 4
        */
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(4))) / (tr(3)));
        let mut vote_simulator = VoteSimulator::new(1);
        vote_simulator.fill_bank_forks(forks, &HashMap::new());
        let bank_forks = vote_simulator.bank_forks;
        let mut frozen_banks: Vec<_> = bank_forks
            .read()
            .unwrap()
            .frozen_banks()
            .values()
            .cloned()
            .collect();
        frozen_banks.sort_by_key(|bank| bank.slot());

        let root = bank_forks.read().unwrap().root();
        let heaviest_subtree_fork_choice =
            HeaviestSubtreeForkChoice::new_from_frozen_banks(root, &frozen_banks);
        assert!(heaviest_subtree_fork_choice.parent(0).is_none());
        assert_eq!(heaviest_subtree_fork_choice.children(0).unwrap(), &[1]);
        assert_eq!(heaviest_subtree_fork_choice.parent(1), Some(0));
        assert_eq!(heaviest_subtree_fork_choice.children(1).unwrap(), &[2, 3]);
        assert_eq!(heaviest_subtree_fork_choice.parent(2), Some(1));
        assert_eq!(heaviest_subtree_fork_choice.children(2).unwrap(), &[4]);
        assert_eq!(heaviest_subtree_fork_choice.parent(3), Some(1));
        assert!(heaviest_subtree_fork_choice.children(3).unwrap().is_empty());
        assert_eq!(heaviest_subtree_fork_choice.parent(4), Some(2));
        assert!(heaviest_subtree_fork_choice.children(4).unwrap().is_empty());
    }

    #[test]
    fn test_set_root() {
        let mut heaviest_subtree_fork_choice = setup_forks();

        // Set root to 1, should only purge 0
        heaviest_subtree_fork_choice.set_root(1);
        for i in 0..=6 {
            let exists = i != 0;
            assert_eq!(
                heaviest_subtree_fork_choice.fork_infos.contains_key(&i),
                exists
            );
        }

        // Set root to 5, should purge everything except 5, 6
        heaviest_subtree_fork_choice.set_root(5);
        for i in 0..=6 {
            let exists = i == 5 || i == 6;
            assert_eq!(
                heaviest_subtree_fork_choice.fork_infos.contains_key(&i),
                exists
            );
        }
    }

    #[test]
    fn test_propagate_new_leaf() {
        let mut heaviest_subtree_fork_choice = setup_forks();
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator(6)
            .collect::<Vec<_>>();
        for a in ancestors.into_iter().chain(std::iter::once(6)) {
            assert_eq!(heaviest_subtree_fork_choice.best_slot(a).unwrap(), 6);
        }

        // Add a leaf 10, it should be the best choice
        heaviest_subtree_fork_choice.add_new_leaf_slot(10, Some(6));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator(10)
            .collect::<Vec<_>>();
        for a in ancestors.into_iter().chain(std::iter::once(10)) {
            assert_eq!(heaviest_subtree_fork_choice.best_slot(a).unwrap(), 10);
        }

        // Add a smaller leaf 9, it should be the best choice
        heaviest_subtree_fork_choice.add_new_leaf_slot(9, Some(6));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator(9)
            .collect::<Vec<_>>();
        for a in ancestors.into_iter().chain(std::iter::once(9)) {
            assert_eq!(heaviest_subtree_fork_choice.best_slot(a).unwrap(), 9);
        }

        // Add a higher leaf 11, should not change the best choice
        heaviest_subtree_fork_choice.add_new_leaf_slot(11, Some(6));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator(11)
            .collect::<Vec<_>>();
        for a in ancestors.into_iter().chain(std::iter::once(9)) {
            assert_eq!(heaviest_subtree_fork_choice.best_slot(a).unwrap(), 9);
        }

        // If an earlier ancestor than the direct parent already has another `best_slot`,
        // it shouldn't change.
        let stake = 100;
        let (bank, vote_pubkeys) = setup_bank_and_vote_pubkeys(3, stake);
        let other_best_leaf = 4;
        // Leaf slot 9 stops being the `best_slot` at slot 1 b/c there are votes
        // for slot 2
        for pubkey in &vote_pubkeys[0..1] {
            heaviest_subtree_fork_choice.add_votes(
                &[(*pubkey, other_best_leaf)],
                bank.epoch_stakes_map(),
                bank.epoch_schedule(),
            );
        }
        heaviest_subtree_fork_choice.add_new_leaf_slot(8, Some(6));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator(8)
            .collect::<Vec<_>>();
        for a in ancestors.into_iter().chain(std::iter::once(8)) {
            let best_slot = if a > 1 { 8 } else { other_best_leaf };
            assert_eq!(
                heaviest_subtree_fork_choice.best_slot(a).unwrap(),
                best_slot
            );
        }

        // If the direct parent's `best_slot` has non-zero stake voting
        // for it, then the `best_slot` should not change, even with a lower
        // leaf being added
        assert_eq!(heaviest_subtree_fork_choice.best_slot(6).unwrap(), 8);
        // Add a vote for slot 8
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[2], 8)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        heaviest_subtree_fork_choice.add_new_leaf_slot(7, Some(6));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator(7)
            .collect::<Vec<_>>();
        // best_slot's remain unchanged
        for a in ancestors.into_iter().chain(std::iter::once(8)) {
            let best_slot = if a > 1 { 8 } else { other_best_leaf };
            assert_eq!(
                heaviest_subtree_fork_choice.best_slot(a).unwrap(),
                best_slot
            );
        }

        // All the leaves should think they are their own best choice
        for leaf in [8, 9, 10, 11].iter() {
            assert_eq!(
                heaviest_subtree_fork_choice.best_slot(*leaf).unwrap(),
                *leaf
            );
        }
    }

    #[test]
    fn test_propagate_new_leaf_2() {
        /*
            Build fork structure:
                 slot 0
                   |
                 slot 4
                   |
                 slot 5
        */
        let forks = tr(0) / (tr(4) / (tr(5)));
        let mut heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new_from_tree(forks);

        let stake = 100;
        let (bank, vote_pubkeys) = setup_bank_and_vote_pubkeys(1, stake);

        // slot 5 should be the best because it's the only leaef
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 5);

        // Add a leaf slot 2 on a different fork than slot 5. Slot 2 should
        // be the new best because it's for a lesser slot
        heaviest_subtree_fork_choice.add_new_leaf_slot(2, Some(0));
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 2);

        // Add a vote for slot 4
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[0], 4)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // slot 5 should be the best again
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 5);

        // Adding a slot 1 that is less than the current best slot 5 should not change the best
        // slot because the fork slot 5 is on has a higher weight
        heaviest_subtree_fork_choice.add_new_leaf_slot(1, Some(0));
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 5);
    }

    #[test]
    fn test_aggregate_slot() {
        let mut heaviest_subtree_fork_choice = setup_forks();

        // No weights are present, weights should be zero
        heaviest_subtree_fork_choice.aggregate_slot(1);
        assert_eq!(heaviest_subtree_fork_choice.stake_voted_at(1).unwrap(), 0);
        assert_eq!(
            heaviest_subtree_fork_choice.stake_voted_subtree(1).unwrap(),
            0
        );
        // The best leaf when weights are equal should prioritize the lower leaf
        assert_eq!(heaviest_subtree_fork_choice.best_slot(3).unwrap(), 6);
        assert_eq!(heaviest_subtree_fork_choice.best_slot(2).unwrap(), 4);
        assert_eq!(heaviest_subtree_fork_choice.best_slot(1).unwrap(), 4);

        // Update the weights that have voted *exactly* at each slot
        heaviest_subtree_fork_choice.set_stake_voted_at(6, 3);
        heaviest_subtree_fork_choice.set_stake_voted_at(5, 3);
        heaviest_subtree_fork_choice.set_stake_voted_at(2, 4);
        heaviest_subtree_fork_choice.set_stake_voted_at(4, 2);
        let total_stake = 12;

        // Aggregate up each off the two forks (order matters, has to be
        // reverse order for each fork, and aggregating a slot multiple times
        // is fine)
        let slots_to_aggregate: Vec<_> = std::iter::once(6)
            .chain(heaviest_subtree_fork_choice.ancestor_iterator(6))
            .chain(std::iter::once(4))
            .chain(heaviest_subtree_fork_choice.ancestor_iterator(4))
            .collect();

        for slot in slots_to_aggregate {
            heaviest_subtree_fork_choice.aggregate_slot(slot);
        }

        for slot in 0..=1 {
            // No stake has voted exactly at slots 0 or 1
            assert_eq!(
                heaviest_subtree_fork_choice.stake_voted_at(slot).unwrap(),
                0
            );
            // Subtree stake is sum of thee `stake_voted_at` across
            // all slots in the subtree
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(slot)
                    .unwrap(),
                total_stake
            );
            // The best path is 0 -> 1 -> 2 -> 4, so slot 4 should be the best choice
            assert_eq!(heaviest_subtree_fork_choice.best_slot(slot).unwrap(), 4);
        }
    }

    #[test]
    fn test_process_update_operations() {
        let mut heaviest_subtree_fork_choice = setup_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = setup_bank_and_vote_pubkeys(3, stake);

        let pubkey_votes: Vec<(Pubkey, Slot)> = vec![
            (vote_pubkeys[0], 3),
            (vote_pubkeys[1], 2),
            (vote_pubkeys[2], 1),
        ];
        let expected_best_slot =
            |slot, heaviest_subtree_fork_choice: &HeaviestSubtreeForkChoice| -> Slot {
                if !heaviest_subtree_fork_choice.is_leaf(slot) {
                    // Both branches have equal weight, so should pick the lesser leaf
                    if heaviest_subtree_fork_choice
                        .ancestor_iterator(4)
                        .collect::<HashSet<Slot>>()
                        .contains(&slot)
                    {
                        4
                    } else {
                        6
                    }
                } else {
                    slot
                }
            };

        check_process_update_correctness(
            &mut heaviest_subtree_fork_choice,
            &pubkey_votes,
            0..7,
            &bank,
            stake,
            expected_best_slot,
        );

        // Everyone makes newer votes
        let pubkey_votes: Vec<(Pubkey, Slot)> = vec![
            (vote_pubkeys[0], 4),
            (vote_pubkeys[1], 3),
            (vote_pubkeys[2], 3),
        ];

        let expected_best_slot =
            |slot, heaviest_subtree_fork_choice: &HeaviestSubtreeForkChoice| -> Slot {
                if !heaviest_subtree_fork_choice.is_leaf(slot) {
                    // The branch with leaf 6 now has two votes, so should pick that one
                    if heaviest_subtree_fork_choice
                        .ancestor_iterator(6)
                        .collect::<HashSet<Slot>>()
                        .contains(&slot)
                    {
                        6
                    } else {
                        4
                    }
                } else {
                    slot
                }
            };

        check_process_update_correctness(
            &mut heaviest_subtree_fork_choice,
            &pubkey_votes,
            0..7,
            &bank,
            stake,
            expected_best_slot,
        );
    }

    #[test]
    fn test_generate_update_operations() {
        let mut heaviest_subtree_fork_choice = setup_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = setup_bank_and_vote_pubkeys(3, stake);
        let pubkey_votes: Vec<(Pubkey, Slot)> = vec![
            (vote_pubkeys[0], 3),
            (vote_pubkeys[1], 4),
            (vote_pubkeys[2], 1),
        ];

        let expected_update_operations: BTreeMap<(Slot, UpdateLabel), UpdateOperation> = vec![
            // Add/remove from new/old forks
            ((1, UpdateLabel::Add), UpdateOperation::Add(stake)),
            ((3, UpdateLabel::Add), UpdateOperation::Add(stake)),
            ((4, UpdateLabel::Add), UpdateOperation::Add(stake)),
            // Aggregate all ancestors of changed slots
            ((0, UpdateLabel::Aggregate), UpdateOperation::Aggregate),
            ((1, UpdateLabel::Aggregate), UpdateOperation::Aggregate),
            ((2, UpdateLabel::Aggregate), UpdateOperation::Aggregate),
        ]
        .into_iter()
        .collect();

        let generated_update_operations = heaviest_subtree_fork_choice.generate_update_operations(
            &pubkey_votes,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(expected_update_operations, generated_update_operations);

        // Everyone makes older/same votes, should be ignored
        let pubkey_votes: Vec<(Pubkey, Slot)> = vec![
            (vote_pubkeys[0], 3),
            (vote_pubkeys[1], 2),
            (vote_pubkeys[2], 1),
        ];
        let generated_update_operations = heaviest_subtree_fork_choice.generate_update_operations(
            &pubkey_votes,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert!(generated_update_operations.is_empty());

        // Some people make newer votes
        let pubkey_votes: Vec<(Pubkey, Slot)> = vec![
            // old, ignored
            (vote_pubkeys[0], 3),
            // new, switched forks
            (vote_pubkeys[1], 5),
            // new, same fork
            (vote_pubkeys[2], 3),
        ];

        let expected_update_operations: BTreeMap<(Slot, UpdateLabel), UpdateOperation> = vec![
            // Add/remove to/from new/old forks
            ((3, UpdateLabel::Add), UpdateOperation::Add(stake)),
            ((5, UpdateLabel::Add), UpdateOperation::Add(stake)),
            ((1, UpdateLabel::Subtract), UpdateOperation::Subtract(stake)),
            ((4, UpdateLabel::Subtract), UpdateOperation::Subtract(stake)),
            // Aggregate all ancestors of changed slots
            ((0, UpdateLabel::Aggregate), UpdateOperation::Aggregate),
            ((1, UpdateLabel::Aggregate), UpdateOperation::Aggregate),
            ((2, UpdateLabel::Aggregate), UpdateOperation::Aggregate),
            ((3, UpdateLabel::Aggregate), UpdateOperation::Aggregate),
        ]
        .into_iter()
        .collect();

        let generated_update_operations = heaviest_subtree_fork_choice.generate_update_operations(
            &pubkey_votes,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(expected_update_operations, generated_update_operations);

        // People make new votes
        let pubkey_votes: Vec<(Pubkey, Slot)> = vec![
            // new, switch forks
            (vote_pubkeys[0], 4),
            // new, same fork
            (vote_pubkeys[1], 6),
            // new, same fork
            (vote_pubkeys[2], 6),
        ];

        let expected_update_operations: BTreeMap<(Slot, UpdateLabel), UpdateOperation> = vec![
            // Add/remove from new/old forks
            ((4, UpdateLabel::Add), UpdateOperation::Add(stake)),
            ((6, UpdateLabel::Add), UpdateOperation::Add(2 * stake)),
            (
                (3, UpdateLabel::Subtract),
                UpdateOperation::Subtract(2 * stake),
            ),
            ((5, UpdateLabel::Subtract), UpdateOperation::Subtract(stake)),
            // Aggregate all ancestors of changed slots
            ((0, UpdateLabel::Aggregate), UpdateOperation::Aggregate),
            ((1, UpdateLabel::Aggregate), UpdateOperation::Aggregate),
            ((2, UpdateLabel::Aggregate), UpdateOperation::Aggregate),
            ((3, UpdateLabel::Aggregate), UpdateOperation::Aggregate),
            ((5, UpdateLabel::Aggregate), UpdateOperation::Aggregate),
        ]
        .into_iter()
        .collect();

        let generated_update_operations = heaviest_subtree_fork_choice.generate_update_operations(
            &pubkey_votes,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(expected_update_operations, generated_update_operations);
    }

    #[test]
    fn test_add_votes() {
        let mut heaviest_subtree_fork_choice = setup_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = setup_bank_and_vote_pubkeys(3, stake);

        let pubkey_votes: Vec<(Pubkey, Slot)> = vec![
            (vote_pubkeys[0], 3),
            (vote_pubkeys[1], 2),
            (vote_pubkeys[2], 1),
        ];
        assert_eq!(
            heaviest_subtree_fork_choice.add_votes(
                &pubkey_votes,
                bank.epoch_stakes_map(),
                bank.epoch_schedule()
            ),
            4
        );

        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 4)
    }

    fn setup_forks() -> HeaviestSubtreeForkChoice {
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
                      |
                    slot 6
        */
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(4))) / (tr(3) / (tr(5) / (tr(6)))));
        HeaviestSubtreeForkChoice::new_from_tree(forks)
    }

    fn setup_bank_and_vote_pubkeys(num_vote_accounts: usize, stake: u64) -> (Bank, Vec<Pubkey>) {
        // Create some voters at genesis
        let validator_voting_keypairs: Vec<_> = (0..num_vote_accounts)
            .map(|_| ValidatorVoteKeypairs::new(Keypair::new(), Keypair::new(), Keypair::new()))
            .collect();

        let vote_pubkeys: Vec<_> = validator_voting_keypairs
            .iter()
            .map(|k| k.vote_keypair.pubkey())
            .collect();
        let GenesisConfigInfo { genesis_config, .. } =
            genesis_utils::create_genesis_config_with_vote_accounts(
                10_000,
                &validator_voting_keypairs,
                stake,
            );
        let bank = Bank::new(&genesis_config);
        (bank, vote_pubkeys)
    }

    fn check_process_update_correctness<F>(
        heaviest_subtree_fork_choice: &mut HeaviestSubtreeForkChoice,
        pubkey_votes: &[(Pubkey, Slot)],
        slots_range: Range<Slot>,
        bank: &Bank,
        stake: u64,
        mut expected_best_slot: F,
    ) where
        F: FnMut(Slot, &HeaviestSubtreeForkChoice) -> Slot,
    {
        let unique_votes: HashSet<Slot> = pubkey_votes
            .iter()
            .map(|(_, vote_slot)| *vote_slot)
            .collect();
        let vote_ancestors: HashMap<Slot, HashSet<Slot>> = unique_votes
            .iter()
            .map(|v| {
                (
                    *v,
                    heaviest_subtree_fork_choice.ancestor_iterator(*v).collect(),
                )
            })
            .collect();
        let mut vote_count: HashMap<Slot, usize> = HashMap::new();
        for (_, vote) in pubkey_votes {
            vote_count.entry(*vote).and_modify(|c| *c += 1).or_insert(1);
        }

        // Maps a slot to the number of descendants of that slot
        // that have been voted on
        let num_voted_descendants: HashMap<Slot, usize> = slots_range
            .clone()
            .map(|slot| {
                let num_voted_descendants = vote_ancestors
                    .iter()
                    .map(|(vote_slot, ancestors)| {
                        (ancestors.contains(&slot) || *vote_slot == slot) as usize
                            * vote_count.get(vote_slot).unwrap()
                    })
                    .sum();
                (slot, num_voted_descendants)
            })
            .collect();

        let update_operations = heaviest_subtree_fork_choice.generate_update_operations(
            &pubkey_votes,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        heaviest_subtree_fork_choice.process_update_operations(update_operations);
        for slot in slots_range {
            let expected_stake_voted_at =
                vote_count.get(&slot).cloned().unwrap_or(0) as u64 * stake;
            let expected_stake_voted_subtree =
                *num_voted_descendants.get(&slot).unwrap() as u64 * stake;
            assert_eq!(
                expected_stake_voted_at,
                heaviest_subtree_fork_choice.stake_voted_at(slot).unwrap()
            );
            assert_eq!(
                expected_stake_voted_subtree,
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(slot)
                    .unwrap()
            );
            assert_eq!(
                expected_best_slot(slot, heaviest_subtree_fork_choice),
                heaviest_subtree_fork_choice.best_slot(slot).unwrap()
            );
        }
    }
}

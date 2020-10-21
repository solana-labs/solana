use crate::{
    consensus::{ComputedBankState, Tower},
    fork_choice::ForkChoice,
    progress_map::ProgressMap,
    tree_diff::TreeDiff,
};
use solana_runtime::{bank::Bank, bank_forks::BankForks, epoch_stakes::EpochStakes};
use solana_sdk::{
    clock::{Epoch, Slot},
    epoch_schedule::EpochSchedule,
    pubkey::Pubkey,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::{Arc, RwLock},
    time::Instant,
};
#[cfg(test)]
use trees::{Tree, TreeWalk};

pub type ForkWeight = u64;
const MAX_ROOT_PRINT_SECONDS: u64 = 30;

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

pub struct HeaviestSubtreeForkChoice {
    fork_infos: HashMap<Slot, ForkInfo>,
    latest_votes: HashMap<Pubkey, Slot>,
    root: Slot,
    last_root_time: Instant,
}

impl HeaviestSubtreeForkChoice {
    pub(crate) fn new(root: Slot) -> Self {
        let mut heaviest_subtree_fork_choice = Self {
            root,
            // Doesn't implement default because `root` must
            // exist in all the fields
            fork_infos: HashMap::new(),
            latest_votes: HashMap::new(),
            last_root_time: Instant::now(),
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

    pub fn root(&self) -> Slot {
        self.root
    }

    pub fn max_by_weight(&self, slot1: Slot, slot2: Slot) -> std::cmp::Ordering {
        let weight1 = self.stake_voted_subtree(slot1).unwrap();
        let weight2 = self.stake_voted_subtree(slot2).unwrap();
        if weight1 == weight2 {
            slot1.cmp(&slot2).reverse()
        } else {
            weight1.cmp(&weight2)
        }
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

    pub fn set_root(&mut self, new_root: Slot) {
        // Remove everything reachable from `self.root` but not `new_root`,
        // as those are now unrooted.
        let remove_set = self.subtree_diff(self.root, new_root);
        for slot in remove_set {
            self.fork_infos
                .remove(&slot)
                .expect("Slots reachable from old root must exist in tree");
        }
        self.fork_infos
            .get_mut(&new_root)
            .expect("new root must exist in fork_infos map")
            .parent = None;
        self.root = new_root;
    }

    pub fn add_root_parent(&mut self, root_parent: Slot) {
        assert!(root_parent < self.root);
        assert!(self.fork_infos.get(&root_parent).is_none());
        let root_info = self
            .fork_infos
            .get_mut(&self.root)
            .expect("entry for root must exist");
        root_info.parent = Some(root_parent);
        let root_parent_info = ForkInfo {
            stake_voted_at: 0,
            stake_voted_subtree: root_info.stake_voted_subtree,
            // The `best_slot` of a leaf is itself
            best_slot: root_info.best_slot,
            children: vec![self.root],
            parent: None,
        };
        self.fork_infos.insert(root_parent, root_parent_info);
        self.root = root_parent;
    }

    pub fn add_new_leaf_slot(&mut self, slot: Slot, parent: Option<Slot>) {
        if self.last_root_time.elapsed().as_secs() > MAX_ROOT_PRINT_SECONDS {
            self.print_state();
            self.last_root_time = Instant::now();
        }

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

    // Returns if the given `maybe_best_child` is the heaviest among the children
    // it's parent
    fn is_best_child(&self, maybe_best_child: Slot) -> bool {
        let maybe_best_child_weight = self.stake_voted_subtree(maybe_best_child).unwrap();
        let parent = self.parent(maybe_best_child);
        // If there's no parent, this must be the root
        if parent.is_none() {
            return true;
        }
        for child in self.children(parent.unwrap()).unwrap() {
            let child_weight = self
                .stake_voted_subtree(*child)
                .expect("child must exist in `self.fork_infos`");
            if child_weight > maybe_best_child_weight
                || (maybe_best_child_weight == child_weight && *child < maybe_best_child)
            {
                return false;
            }
        }

        true
    }
    pub fn all_slots_stake_voted_subtree(&self) -> Vec<(Slot, u64)> {
        self.fork_infos
            .iter()
            .map(|(slot, fork_info)| (*slot, fork_info.stake_voted_subtree))
            .collect()
    }

    pub fn ancestors(&self, start_slot: Slot) -> Vec<Slot> {
        AncestorIterator::new(start_slot, &self.fork_infos).collect()
    }

    pub fn merge(
        &mut self,
        other: HeaviestSubtreeForkChoice,
        merge_leaf: Slot,
        epoch_stakes: &HashMap<Epoch, EpochStakes>,
        epoch_schedule: &EpochSchedule,
    ) {
        assert!(self.fork_infos.contains_key(&merge_leaf));

        // Add all the nodes from `other` into our tree
        let mut other_slots_nodes: Vec<_> = other
            .fork_infos
            .iter()
            .map(|(slot, fork_info)| (slot, fork_info.parent.unwrap_or(merge_leaf)))
            .collect();

        other_slots_nodes.sort_by_key(|(slot, _)| *slot);
        for (slot, parent) in other_slots_nodes {
            self.add_new_leaf_slot(*slot, Some(parent));
        }

        // Add all the latest votes from `other` that are newer than the ones
        // in the current tree
        let new_votes: Vec<_> = other
            .latest_votes
            .into_iter()
            .filter(|(pk, other_latest_slot)| {
                self.latest_votes
                    .get(&pk)
                    .map(|latest_slot| other_latest_slot > latest_slot)
                    .unwrap_or(false)
            })
            .collect();

        self.add_votes(&new_votes, epoch_stakes, epoch_schedule);
    }

    pub fn stake_voted_at(&self, slot: Slot) -> Option<u64> {
        self.fork_infos
            .get(&slot)
            .map(|fork_info| fork_info.stake_voted_at)
    }

    fn propagate_new_leaf(&mut self, slot: Slot, parent: Slot) {
        let parent_best_slot = self
            .best_slot(parent)
            .expect("parent must exist in self.fork_infos after its child leaf was created");

        // If this new leaf is the direct parent's best child, then propagate
        // it up the tree
        if self.is_best_child(slot) {
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
            for &child in &fork_info.children {
                let child_stake_voted_subtree = self.stake_voted_subtree(child).unwrap();
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

    fn parent(&self, slot: Slot) -> Option<Slot> {
        self.fork_infos
            .get(&slot)
            .map(|fork_info| fork_info.parent)
            .unwrap_or(None)
    }

    fn print_state(&self) {
        let best_slot = self.best_overall_slot();
        let mut best_path: VecDeque<_> = self.ancestor_iterator(best_slot).collect();
        best_path.push_front(best_slot);
        info!(
            "Latest known votes by vote pubkey: {:#?}, best path: {:?}",
            self.latest_votes,
            best_path.iter().rev().collect::<Vec<&Slot>>()
        );
    }

    fn heaviest_slot_on_same_voted_fork(&self, tower: &Tower) -> Option<Slot> {
        tower
            .last_voted_slot()
            .map(|last_voted_slot| {
                let heaviest_slot_on_same_voted_fork = self.best_slot(last_voted_slot);
                if heaviest_slot_on_same_voted_fork.is_none() {
                    if !tower.is_stray_last_vote() {
                        // Unless last vote is stray and stale, self.bast_slot(last_voted_slot) must return
                        // Some(_), justifying to panic! here.
                        // Also, adjust_lockouts_after_replay() correctly makes last_voted_slot None,
                        // if all saved votes are ancestors of replayed_root_slot. So this code shouldn't be
                        // touched in that case as well.
                        // In other words, except being stray, all other slots have been voted on while this
                        // validator has been running, so we must be able to fetch best_slots for all of
                        // them.
                        panic!(
                            "a bank at last_voted_slot({}) is a frozen bank so must have been \
                            added to heaviest_subtree_fork_choice at time of freezing",
                            last_voted_slot,
                        )
                    } else {
                        // fork_infos doesn't have corresponding data for the stale stray last vote,
                        // meaning some inconsistency between saved tower and ledger.
                        // (newer snapshot, or only a saved tower is moved over to new setup?)
                        return None;
                    }
                }
                let heaviest_slot_on_same_voted_fork = heaviest_slot_on_same_voted_fork.unwrap();

                if heaviest_slot_on_same_voted_fork == last_voted_slot {
                    None
                } else {
                    Some(heaviest_slot_on_same_voted_fork)
                }
            })
            .unwrap_or(None)
    }

    #[cfg(test)]
    fn set_stake_voted_at(&mut self, slot: Slot, stake_voted_at: u64) {
        self.fork_infos.get_mut(&slot).unwrap().stake_voted_at = stake_voted_at;
    }

    #[cfg(test)]
    fn is_leaf(&self, slot: Slot) -> bool {
        self.fork_infos.get(&slot).unwrap().children.is_empty()
    }
}

impl TreeDiff for HeaviestSubtreeForkChoice {
    fn contains_slot(&self, slot: Slot) -> bool {
        self.fork_infos.contains_key(&slot)
    }

    fn children(&self, slot: Slot) -> Option<&[Slot]> {
        self.fork_infos
            .get(&slot)
            .map(|fork_info| &fork_info.children[..])
    }
}

impl ForkChoice for HeaviestSubtreeForkChoice {
    fn compute_bank_stats(
        &mut self,
        bank: &Bank,
        _tower: &Tower,
        _progress: &mut ProgressMap,
        computed_bank_state: &ComputedBankState,
    ) {
        let ComputedBankState { pubkey_votes, .. } = computed_bank_state;

        // Update `heaviest_subtree_fork_choice` to find the best fork to build on
        let best_overall_slot = self.add_votes(
            &pubkey_votes,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        datapoint_info!(
            "best_slot",
            ("slot", bank.slot(), i64),
            ("best_slot", best_overall_slot, i64),
        );
    }

    // Returns:
    // 1) The heaviest overall bank
    // 2) The heaviest bank on the same fork as the last vote (doesn't require a
    // switching proof to vote for)
    fn select_forks(
        &self,
        _frozen_banks: &[Arc<Bank>],
        tower: &Tower,
        _progress: &ProgressMap,
        _ancestors: &HashMap<u64, HashSet<u64>>,
        bank_forks: &RwLock<BankForks>,
    ) -> (Arc<Bank>, Option<Arc<Bank>>) {
        let r_bank_forks = bank_forks.read().unwrap();

        (
            r_bank_forks.get(self.best_overall_slot()).unwrap().clone(),
            self.heaviest_slot_on_same_voted_fork(tower)
                .map(|heaviest_slot_on_same_voted_fork| {
                    r_bank_forks
                        .get(heaviest_slot_on_same_voted_fork)
                        .unwrap()
                        .clone()
                }),
        )
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
    use solana_runtime::{bank::Bank, bank_utils};
    use solana_sdk::{hash::Hash, slot_history::SlotHistory};
    use std::{collections::HashSet, ops::Range};
    use trees::tr;

    #[test]
    fn test_max_by_weight() {
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
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(1, stake);
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[0], 4)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        assert_eq!(
            heaviest_subtree_fork_choice.max_by_weight(4, 5),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            heaviest_subtree_fork_choice.max_by_weight(4, 0),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn test_add_root_parent() {
        /*
            Build fork structure:
                 slot 3
                   |
                 slot 4
                   |
                 slot 5
        */
        let forks = tr(3) / (tr(4) / (tr(5)));
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(1, stake);
        let mut heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new_from_tree(forks);
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[0], 5)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        heaviest_subtree_fork_choice.add_root_parent(2);
        assert_eq!(heaviest_subtree_fork_choice.parent(3).unwrap(), 2);
        assert_eq!(
            heaviest_subtree_fork_choice.stake_voted_subtree(3).unwrap(),
            stake
        );
        assert_eq!(heaviest_subtree_fork_choice.stake_voted_at(2).unwrap(), 0);
        assert_eq!(
            heaviest_subtree_fork_choice.children(2).unwrap().to_vec(),
            vec![3]
        );
        assert_eq!(heaviest_subtree_fork_choice.best_slot(2).unwrap(), 5);
        assert!(heaviest_subtree_fork_choice.parent(2).is_none());
    }

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
    fn test_set_root_and_add_votes() {
        let mut heaviest_subtree_fork_choice = setup_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(1, stake);

        // Vote for slot 2
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[0], 1)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 4);

        // Set a root
        heaviest_subtree_fork_choice.set_root(1);

        // Vote again for slot 3 on a different fork than the last vote,
        // verify this fork is now the best fork
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[0], 3)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 6);
        assert_eq!(heaviest_subtree_fork_choice.stake_voted_at(1).unwrap(), 0);
        assert_eq!(
            heaviest_subtree_fork_choice.stake_voted_at(3).unwrap(),
            stake
        );
        for slot in &[1, 3] {
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(*slot)
                    .unwrap(),
                stake
            );
        }

        // Set a root at last vote
        heaviest_subtree_fork_choice.set_root(3);
        // Check new leaf 7 is still propagated properly
        heaviest_subtree_fork_choice.add_new_leaf_slot(7, Some(6));
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 7);
    }

    #[test]
    fn test_set_root_and_add_outdated_votes() {
        let mut heaviest_subtree_fork_choice = setup_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(1, stake);

        // Vote for slot 0
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[0], 0)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Set root to 1, should purge 0 from the tree, but
        // there's still an outstanding vote for slot 0 in `pubkey_votes`.
        heaviest_subtree_fork_choice.set_root(1);

        // Vote again for slot 3, verify everything is ok
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[0], 3)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(
            heaviest_subtree_fork_choice.stake_voted_at(3).unwrap(),
            stake
        );
        for slot in &[1, 3] {
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(*slot)
                    .unwrap(),
                stake
            );
        }
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 6);

        // Set root again on different fork than the last vote
        heaviest_subtree_fork_choice.set_root(2);
        // Smaller vote than last vote 3 should be ignored
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[0], 2)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(heaviest_subtree_fork_choice.stake_voted_at(2).unwrap(), 0);
        assert_eq!(
            heaviest_subtree_fork_choice.stake_voted_subtree(2).unwrap(),
            0
        );
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 4);

        // New larger vote than last vote 3 should be processed
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[0], 4)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(heaviest_subtree_fork_choice.stake_voted_at(2).unwrap(), 0);
        assert_eq!(
            heaviest_subtree_fork_choice.stake_voted_at(4).unwrap(),
            stake
        );
        assert_eq!(
            heaviest_subtree_fork_choice.stake_voted_subtree(2).unwrap(),
            stake
        );
        assert_eq!(
            heaviest_subtree_fork_choice.stake_voted_subtree(4).unwrap(),
            stake
        );
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 4);
    }

    #[test]
    fn test_best_overall_slot() {
        let heaviest_subtree_fork_choice = setup_forks();
        // Best overall path is 0 -> 1 -> 2 -> 4, so best leaf
        // should be 4
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 4);
    }

    #[test]
    fn test_propagate_new_leaf() {
        let mut heaviest_subtree_fork_choice = setup_forks();

        // Add a leaf 10, it should be the best choice
        heaviest_subtree_fork_choice.add_new_leaf_slot(10, Some(4));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator(10)
            .collect::<Vec<_>>();
        for a in ancestors.into_iter().chain(std::iter::once(10)) {
            assert_eq!(heaviest_subtree_fork_choice.best_slot(a).unwrap(), 10);
        }

        // Add a smaller leaf 9, it should be the best choice
        heaviest_subtree_fork_choice.add_new_leaf_slot(9, Some(4));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator(9)
            .collect::<Vec<_>>();
        for a in ancestors.into_iter().chain(std::iter::once(9)) {
            assert_eq!(heaviest_subtree_fork_choice.best_slot(a).unwrap(), 9);
        }

        // Add a higher leaf 11, should not change the best choice
        heaviest_subtree_fork_choice.add_new_leaf_slot(11, Some(4));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator(11)
            .collect::<Vec<_>>();
        for a in ancestors.into_iter().chain(std::iter::once(9)) {
            assert_eq!(heaviest_subtree_fork_choice.best_slot(a).unwrap(), 9);
        }

        // Add a vote for the other branch at slot 3.
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(2, stake);
        let leaf6 = 6;
        // Leaf slot 9 stops being the `best_slot` at slot 1 because there
        // are now votes for the branch at slot 3
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[0], leaf6)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Because slot 1 now sees the child branch at slot 3 has non-zero
        // weight, adding smaller leaf slot 8 in the other child branch at slot 2
        // should not propagate past slot 1
        heaviest_subtree_fork_choice.add_new_leaf_slot(8, Some(4));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator(8)
            .collect::<Vec<_>>();
        for a in ancestors.into_iter().chain(std::iter::once(8)) {
            let best_slot = if a > 1 { 8 } else { leaf6 };
            assert_eq!(
                heaviest_subtree_fork_choice.best_slot(a).unwrap(),
                best_slot
            );
        }

        // Add vote for slot 8, should now be the best slot (has same weight
        // as fork containing slot 6, but slot 2 is smaller than slot 3).
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[1], 8)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 8);

        // Because slot 4 now sees the child leaf 8 has non-zero
        // weight, adding smaller leaf slots should not propagate past slot 4
        heaviest_subtree_fork_choice.add_new_leaf_slot(7, Some(4));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator(7)
            .collect::<Vec<_>>();
        for a in ancestors.into_iter().chain(std::iter::once(8)) {
            assert_eq!(heaviest_subtree_fork_choice.best_slot(a).unwrap(), 8);
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
                 slot 6
        */
        let forks = tr(0) / (tr(4) / (tr(6)));
        let mut heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new_from_tree(forks);
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(1, stake);

        // slot 6 should be the best because it's the only leaf
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 6);

        // Add a leaf slot 5. Even though 5 is less than the best leaf 6,
        // it's not less than it's sibling slot 4, so the best overall
        // leaf should remain unchanged
        heaviest_subtree_fork_choice.add_new_leaf_slot(5, Some(0));
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 6);

        // Add a leaf slot 2 on a different fork than leaf 6. Slot 2 should
        // be the new best because it's for a lesser slot
        heaviest_subtree_fork_choice.add_new_leaf_slot(2, Some(0));
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 2);

        // Add a vote for slot 4, so leaf 6 should be the best again
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[0], 4)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 6);

        // Adding a slot 1 that is less than the current best leaf 6 should not change the best
        // slot because the fork slot 5 is on has a higher weight
        heaviest_subtree_fork_choice.add_new_leaf_slot(1, Some(0));
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 6);
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
        assert_eq!(heaviest_subtree_fork_choice.best_slot(1).unwrap(), 4);
        assert_eq!(heaviest_subtree_fork_choice.best_slot(2).unwrap(), 4);
        assert_eq!(heaviest_subtree_fork_choice.best_slot(3).unwrap(), 6);

        // Update the weights that have voted *exactly* at each slot, the
        // branch containing slots {5, 6} has weight 11, so should be heavier
        // than the branch containing slots {2, 4}
        let mut total_stake = 0;
        let staked_voted_slots: HashSet<_> = vec![2, 4, 5, 6].into_iter().collect();
        for slot in &staked_voted_slots {
            heaviest_subtree_fork_choice.set_stake_voted_at(*slot, *slot);
            total_stake += *slot;
        }

        // Aggregate up each of the two forks (order matters, has to be
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

        // The best path is now 0 -> 1 -> 3 -> 5 -> 6, so leaf 6
        // should be the best choice
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), 6);

        // Verify `stake_voted_at`
        for slot in 0..=6 {
            let expected_stake = if staked_voted_slots.contains(&slot) {
                slot
            } else {
                0
            };

            assert_eq!(
                heaviest_subtree_fork_choice.stake_voted_at(slot).unwrap(),
                expected_stake
            );
        }

        // Verify `stake_voted_subtree` for common fork
        for slot in &[0, 1] {
            // Subtree stake is sum of the `stake_voted_at` across
            // all slots in the subtree
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(*slot)
                    .unwrap(),
                total_stake
            );
        }
        // Verify `stake_voted_subtree` for fork 1
        let mut total_expected_stake = 0;
        for slot in &[4, 2] {
            total_expected_stake += heaviest_subtree_fork_choice.stake_voted_at(*slot).unwrap();
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(*slot)
                    .unwrap(),
                total_expected_stake
            );
        }
        // Verify `stake_voted_subtree` for fork 2
        total_expected_stake = 0;
        for slot in &[6, 5, 3] {
            total_expected_stake += heaviest_subtree_fork_choice.stake_voted_at(*slot).unwrap();
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(*slot)
                    .unwrap(),
                total_expected_stake
            );
        }
    }

    #[test]
    fn test_process_update_operations() {
        let mut heaviest_subtree_fork_choice = setup_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(3, stake);

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
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(3, stake);
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
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(3, stake);

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

    #[test]
    fn test_is_best_child() {
        /*
            Build fork structure:
                 slot 0
                   |
                 slot 4
                /      \
          slot 10     slot 9
        */
        let forks = tr(0) / (tr(4) / (tr(9)) / (tr(10)));
        let mut heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new_from_tree(forks);
        assert!(heaviest_subtree_fork_choice.is_best_child(0));
        assert!(heaviest_subtree_fork_choice.is_best_child(4));

        // 9 is better than 10
        assert!(heaviest_subtree_fork_choice.is_best_child(9));
        assert!(!heaviest_subtree_fork_choice.is_best_child(10));

        // Add new leaf 8, which is better than 9, as both have weight 0
        heaviest_subtree_fork_choice.add_new_leaf_slot(8, Some(4));
        assert!(heaviest_subtree_fork_choice.is_best_child(8));
        assert!(!heaviest_subtree_fork_choice.is_best_child(9));
        assert!(!heaviest_subtree_fork_choice.is_best_child(10));

        // Add vote for 9, it's the best again
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(3, 100);
        heaviest_subtree_fork_choice.add_votes(
            &[(vote_pubkeys[0], 9)],
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert!(heaviest_subtree_fork_choice.is_best_child(9));
        assert!(!heaviest_subtree_fork_choice.is_best_child(8));
        assert!(!heaviest_subtree_fork_choice.is_best_child(10));
    }

    #[test]
    fn test_merge() {
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(3, stake);
        /*
            Build fork structure:
                 slot 0
                   |
                 slot 3
                 /    \
            slot 5    |
               |    slot 9
            slot 7    |
                    slot 11
                      |
                    slot 12 (vote pubkey 2)
        */
        let forks = tr(0) / (tr(3) / (tr(5) / (tr(7))) / (tr(9) / (tr(11) / (tr(12)))));
        let mut tree1 = HeaviestSubtreeForkChoice::new_from_tree(forks);
        let pubkey_votes: Vec<(Pubkey, Slot)> = vec![
            (vote_pubkeys[0], 5),
            (vote_pubkeys[1], 3),
            (vote_pubkeys[2], 12),
        ];
        tree1.add_votes(
            &pubkey_votes,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        /*
                    Build fork structure:
                          slot 10
                             |
                          slot 15
                         /       \
        (vote pubkey 0) slot 16   |
                       |       slot 18
                    slot 17       |
                               slot 19 (vote pubkey 1)
                                  |
                               slot 20
                */
        let forks = tr(10) / (tr(15) / (tr(16) / (tr(17))) / (tr(18) / (tr(19) / (tr(20)))));
        let mut tree2 = HeaviestSubtreeForkChoice::new_from_tree(forks);
        let pubkey_votes: Vec<(Pubkey, Slot)> = vec![
            // more than tree 1
            (vote_pubkeys[0], 16),
            // more than tree1
            (vote_pubkeys[1], 19),
            // less than tree1
            (vote_pubkeys[2], 10),
        ];
        tree2.add_votes(
            &pubkey_votes,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Merge tree2 at leaf 7 of tree1
        tree1.merge(tree2, 7, bank.epoch_stakes_map(), bank.epoch_schedule());

        // Check ancestry information is correct
        let ancestors: Vec<_> = tree1.ancestor_iterator(20).collect();
        assert_eq!(ancestors, vec![19, 18, 15, 10, 7, 5, 3, 0]);
        let ancestors: Vec<_> = tree1.ancestor_iterator(17).collect();
        assert_eq!(ancestors, vec![16, 15, 10, 7, 5, 3, 0]);

        // Check correctness off votes
        // Pubkey 0
        assert_eq!(tree1.stake_voted_at(16).unwrap(), stake);
        assert_eq!(tree1.stake_voted_at(5).unwrap(), 0);
        // Pubkey 1
        assert_eq!(tree1.stake_voted_at(19).unwrap(), stake);
        assert_eq!(tree1.stake_voted_at(3).unwrap(), 0);
        // Pubkey 2
        assert_eq!(tree1.stake_voted_at(10).unwrap(), 0);
        assert_eq!(tree1.stake_voted_at(12).unwrap(), stake);

        for slot in &[0, 3] {
            assert_eq!(tree1.stake_voted_subtree(*slot).unwrap(), 3 * stake);
        }
        for slot in &[5, 7, 10, 15] {
            assert_eq!(tree1.stake_voted_subtree(*slot).unwrap(), 2 * stake);
        }
        for slot in &[9, 11, 12, 16, 18, 19] {
            assert_eq!(tree1.stake_voted_subtree(*slot).unwrap(), stake);
        }
        for slot in &[17, 20] {
            assert_eq!(tree1.stake_voted_subtree(*slot).unwrap(), 0);
        }

        assert_eq!(tree1.best_overall_slot(), 17);
    }

    #[test]
    fn test_subtree_diff() {
        let mut heaviest_subtree_fork_choice = setup_forks();

        // Diff of same root is empty, no matter root, intermediate node, or leaf
        assert!(heaviest_subtree_fork_choice.subtree_diff(0, 0).is_empty());
        assert!(heaviest_subtree_fork_choice.subtree_diff(5, 5).is_empty());
        assert!(heaviest_subtree_fork_choice.subtree_diff(6, 6).is_empty());

        // The set reachable from slot 3, excluding subtree 1, is just everything
        // in slot 3 since subtree 1 is an ancestor
        assert_eq!(
            heaviest_subtree_fork_choice.subtree_diff(3, 1),
            vec![3, 5, 6].into_iter().collect::<HashSet<_>>()
        );

        // The set reachable from slot 1, excluding subtree 3, is just 1 and
        // the subtree at 2
        assert_eq!(
            heaviest_subtree_fork_choice.subtree_diff(1, 3),
            vec![1, 2, 4].into_iter().collect::<HashSet<_>>()
        );

        // The set reachable from slot 1, excluding leaf 6, is just everything
        // except leaf 6
        assert_eq!(
            heaviest_subtree_fork_choice.subtree_diff(0, 6),
            vec![0, 1, 3, 5, 2, 4].into_iter().collect::<HashSet<_>>()
        );

        // Set root at 1
        heaviest_subtree_fork_choice.set_root(1);

        // Zero no longer exists, set reachable from 0 is empty
        assert!(heaviest_subtree_fork_choice.subtree_diff(0, 6).is_empty());
    }

    #[test]
    fn test_stray_restored_slot() {
        let forks = tr(0) / (tr(1) / tr(2));
        let heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new_from_tree(forks);

        let mut tower = Tower::new_for_tests(10, 0.9);
        tower.record_vote(1, Hash::default());

        assert_eq!(tower.is_stray_last_vote(), false);
        assert_eq!(
            heaviest_subtree_fork_choice.heaviest_slot_on_same_voted_fork(&tower),
            Some(2)
        );

        // Make slot 1 (existing in bank_forks) a restored stray slot
        let mut slot_history = SlotHistory::default();
        slot_history.add(0);
        // Work around TooOldSlotHistory
        slot_history.add(999);
        tower = tower
            .adjust_lockouts_after_replay(0, &slot_history)
            .unwrap();

        assert_eq!(tower.is_stray_last_vote(), true);
        assert_eq!(
            heaviest_subtree_fork_choice.heaviest_slot_on_same_voted_fork(&tower),
            Some(2)
        );

        // Make slot 3 (NOT existing in bank_forks) a restored stray slot
        tower.record_vote(3, Hash::default());
        tower = tower
            .adjust_lockouts_after_replay(0, &slot_history)
            .unwrap();

        assert_eq!(tower.is_stray_last_vote(), true);
        assert_eq!(
            heaviest_subtree_fork_choice.heaviest_slot_on_same_voted_fork(&tower),
            None
        );
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

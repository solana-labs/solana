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
    hash::Hash,
    pubkey::Pubkey,
};
use std::{
    borrow::Borrow,
    cmp::Ordering,
    collections::{hash_map::Entry, BTreeMap, HashMap, HashSet, VecDeque},
    sync::{Arc, RwLock},
    time::Instant,
};
#[cfg(test)]
use trees::{Tree, TreeWalk};

pub type ForkWeight = u64;
type SlotHashKey = (Slot, Hash);

const MAX_ROOT_PRINT_SECONDS: u64 = 30;

#[derive(PartialEq, Eq, Clone, Debug, PartialOrd, Ord)]
enum UpdateLabel {
    Aggregate,
    Add,
    MarkValid,
    Subtract,
}

pub trait GetSlotHash {
    fn slot_hash(&self) -> SlotHashKey;
}

impl GetSlotHash for SlotHashKey {
    fn slot_hash(&self) -> SlotHashKey {
        *self
    }
}

impl GetSlotHash for Slot {
    fn slot_hash(&self) -> SlotHashKey {
        (*self, Hash::default())
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
enum UpdateOperation {
    Aggregate,
    Add(u64),
    MarkValid,
    Subtract(u64),
}

impl UpdateOperation {
    fn update_stake(&mut self, new_stake: u64) {
        match self {
            Self::Aggregate => panic!("Should not get here"),
            Self::Add(stake) => *stake += new_stake,
            Self::MarkValid => panic!("Should not get here"),
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
    best_slot: SlotHashKey,
    parent: Option<SlotHashKey>,
    children: Vec<SlotHashKey>,
    // Whether the fork rooted at this slot is a valid contender
    // for the best fork
    is_candidate: bool,
    duplicate_votes: HashMap<Pubkey, u64>,
}

impl ForkInfo {
    fn get_duplicate_pubkey_stake(&self, pubkey: &Pubkey) -> Option<u64> {
        self.duplicate_votes.get(pubkey).cloned()
    }
}

#[derive(Debug, Default)]
struct LatestVotes {
    slot: Slot,
    // The hash of the first observed block that this validator voted on
    // with slot == `self.slot`
    hashes: HashSet<Hash>,
}

impl LatestVotes {
    fn into_votes(self, pubkey: Pubkey) -> Vec<(Pubkey, SlotHashKey)> {
        self.into_slot_hashes()
            .map(|slot_hash| (pubkey, slot_hash))
            .collect::<Vec<(Pubkey, SlotHashKey)>>()
    }

    fn into_slot_hashes(self) -> impl Iterator<Item = SlotHashKey> {
        let slot = self.slot;
        self.hashes.into_iter().map(move |h| (slot, h))
    }

    fn is_duplicate(&self) -> bool {
        self.hashes.len() > 1
    }
}

pub struct HeaviestSubtreeForkChoice {
    fork_infos: HashMap<SlotHashKey, ForkInfo>,
    latest_votes: HashMap<Pubkey, LatestVotes>,
    root: SlotHashKey,
    last_root_time: Instant,
}

impl HeaviestSubtreeForkChoice {
    pub(crate) fn new(root: SlotHashKey) -> Self {
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
    pub(crate) fn new_from_frozen_banks(root: SlotHashKey, frozen_banks: &[Arc<Bank>]) -> Self {
        let mut heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new(root);
        let mut prev_slot = root.0;
        for bank in frozen_banks.iter() {
            assert!(bank.is_frozen());
            if bank.slot() > root.0 {
                // Make sure the list is sorted
                assert!(bank.slot() > prev_slot);
                prev_slot = bank.slot();
                let bank_hash = bank.hash();
                assert_ne!(bank_hash, Hash::default());
                let parent_bank_hash = bank.parent_hash();
                assert_ne!(parent_bank_hash, Hash::default());
                heaviest_subtree_fork_choice.add_new_leaf_slot(
                    (bank.slot(), bank_hash),
                    Some((bank.parent_slot(), parent_bank_hash)),
                );
            }
        }

        heaviest_subtree_fork_choice
    }

    #[cfg(test)]
    pub(crate) fn new_from_bank_forks(bank_forks: &BankForks) -> Self {
        let mut frozen_banks: Vec<_> = bank_forks.frozen_banks().values().cloned().collect();

        frozen_banks.sort_by_key(|bank| bank.slot());
        let root_bank = bank_forks.root_bank();
        Self::new_from_frozen_banks((root_bank.slot(), root_bank.hash()), &frozen_banks)
    }

    #[cfg(test)]
    pub(crate) fn new_from_tree<T: GetSlotHash>(forks: Tree<T>) -> Self {
        let root = forks.root().data.slot_hash();
        let mut walk = TreeWalk::from(forks);
        let mut heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new(root);
        while let Some(visit) = walk.get() {
            let slot_hash = visit.node().data.slot_hash();
            if heaviest_subtree_fork_choice
                .fork_infos
                .contains_key(&slot_hash)
            {
                walk.forward();
                continue;
            }
            let parent_slot_hash = walk.get_parent().map(|n| n.data.slot_hash());
            heaviest_subtree_fork_choice.add_new_leaf_slot(slot_hash, parent_slot_hash);
            walk.forward();
        }

        heaviest_subtree_fork_choice
    }

    pub fn best_slot(&self, key: &SlotHashKey) -> Option<SlotHashKey> {
        self.fork_infos
            .get(key)
            .map(|fork_info| fork_info.best_slot)
    }

    pub fn best_overall_slot(&self) -> SlotHashKey {
        self.best_slot(&self.root).unwrap()
    }

    pub fn stake_voted_subtree(&self, key: &SlotHashKey) -> Option<u64> {
        self.fork_infos
            .get(key)
            .map(|fork_info| fork_info.stake_voted_subtree)
    }

    pub fn is_candidate_slot(&self, key: &SlotHashKey) -> Option<bool> {
        self.fork_infos
            .get(key)
            .map(|fork_info| fork_info.is_candidate)
    }

    pub fn root(&self) -> SlotHashKey {
        self.root
    }

    #[cfg(test)]
    pub fn latest_voted_slot_hashes(&self, pubkey: &Pubkey) -> Option<Vec<SlotHashKey>> {
        self.latest_votes.get(pubkey).map(|latest_vote| {
            latest_vote
                .hashes
                .iter()
                .map(|hash| (latest_vote.slot, *hash))
                .collect::<Vec<SlotHashKey>>()
        })
    }

    pub fn max_by_weight(&self, slot1: SlotHashKey, slot2: SlotHashKey) -> std::cmp::Ordering {
        let weight1 = self.stake_voted_subtree(&slot1).unwrap();
        let weight2 = self.stake_voted_subtree(&slot2).unwrap();
        if weight1 == weight2 {
            slot1.cmp(&slot2).reverse()
        } else {
            weight1.cmp(&weight2)
        }
    }

    // Add new votes, returns the best slot
    pub fn add_votes<'a, 'b>(
        &'a mut self,
        // newly updated votes on a fork
        pubkey_votes: impl Iterator<Item = impl Borrow<(Pubkey, SlotHashKey)> + 'b>,
        epoch_stakes: &HashMap<Epoch, EpochStakes>,
        epoch_schedule: &EpochSchedule,
    ) -> SlotHashKey {
        // Generate the set of updates
        let (update_operations, duplicate_pubkeys_added, duplicate_pubkeys_removed) =
            self.generate_update_operations(pubkey_votes, epoch_stakes, epoch_schedule);

        // Finalize all updates
        self.process_update_operations(
            update_operations,
            &duplicate_pubkeys_added,
            &duplicate_pubkeys_removed,
        );
        self.best_overall_slot()
    }

    pub fn set_root(&mut self, new_root: SlotHashKey) {
        // Remove everything reachable from `self.root` but not `new_root`,
        // as those are now unrooted.
        let remove_set = self.subtree_diff(self.root, new_root);
        for node_key in remove_set {
            self.fork_infos
                .remove(&node_key)
                .expect("Slots reachable from old root must exist in tree");
        }
        let root_fork_info = self.fork_infos.get_mut(&new_root);

        root_fork_info
            .unwrap_or_else(|| panic!("New root: {:?}, didn't exist in fork choice", new_root))
            .parent = None;
        self.root = new_root;
        self.last_root_time = Instant::now();
    }

    pub fn add_root_parent(&mut self, root_parent: SlotHashKey) {
        assert!(root_parent.0 < self.root.0);
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
            is_candidate: true,
            duplicate_votes: HashMap::new(),
        };
        self.fork_infos.insert(root_parent, root_parent_info);
        self.root = root_parent;
    }

    pub fn add_new_leaf_slot(&mut self, slot: SlotHashKey, parent: Option<SlotHashKey>) {
        if self.last_root_time.elapsed().as_secs() > MAX_ROOT_PRINT_SECONDS {
            self.print_state();
            self.last_root_time = Instant::now();
        }

        if self.fork_infos.contains_key(&slot) {
            // Can potentially happen if we repair the same version of the duplicate slot, after
            // dumping the original version
            return;
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
                is_candidate: true,
                duplicate_votes: HashMap::new(),
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
        self.propagate_new_leaf(&slot, &parent)
    }

    // Returns if the given `maybe_best_child` is the heaviest among the children
    // it's parent
    fn is_best_child(&self, maybe_best_child: &SlotHashKey) -> bool {
        let maybe_best_child_weight = self.stake_voted_subtree(maybe_best_child).unwrap();
        let parent = self.parent(maybe_best_child);
        // If there's no parent, this must be the root
        if parent.is_none() {
            return true;
        }
        for child in self.children(&parent.unwrap()).unwrap() {
            let child_weight = self
                .stake_voted_subtree(child)
                .expect("child must exist in `self.fork_infos`");

            // Don't count children currently marked as invalid
            if !self
                .is_candidate_slot(child)
                .expect("child must exist in tree")
            {
                continue;
            }

            if child_weight > maybe_best_child_weight
                || (maybe_best_child_weight == child_weight && *child < *maybe_best_child)
            {
                return false;
            }
        }

        true
    }

    pub fn all_slots_stake_voted_subtree(&self) -> Vec<(SlotHashKey, u64)> {
        self.fork_infos
            .iter()
            .map(|(slot_hash, fork_info)| (*slot_hash, fork_info.stake_voted_subtree))
            .collect()
    }

    pub fn ancestors(&self, start_slot_hash_key: SlotHashKey) -> Vec<SlotHashKey> {
        AncestorIterator::new(start_slot_hash_key, &self.fork_infos).collect()
    }

    pub fn merge(
        &mut self,
        other: HeaviestSubtreeForkChoice,
        merge_leaf: &SlotHashKey,
        epoch_stakes: &HashMap<Epoch, EpochStakes>,
        epoch_schedule: &EpochSchedule,
    ) {
        assert!(self.fork_infos.contains_key(merge_leaf));

        // Add all the nodes from `other` into our tree
        let mut other_slots_nodes: Vec<_> = other
            .fork_infos
            .iter()
            .map(|(slot_hash_key, fork_info)| {
                (slot_hash_key, fork_info.parent.unwrap_or(*merge_leaf))
            })
            .collect();

        other_slots_nodes.sort_by_key(|(slot_hash_key, _)| *slot_hash_key);
        for (slot_hash_key, parent) in other_slots_nodes {
            self.add_new_leaf_slot(*slot_hash_key, Some(parent));
        }

        // Add all the latest votes from `other` that are newer than the ones
        // in the current tree, or if they are for the same slot, only add the
        // merged vote if it's for a different hash.
        let new_votes: Vec<_> = other
            .latest_votes
            .into_iter()
            .filter_map(
                |(pk, pk_their_latest_vote /*(other_latest_vote_slot, s)*/)| {
                    if let Some(
                        pk_our_latest_vote, /*(latest_vote_slot, latest_slot_hashes)*/
                    ) = self.latest_votes.get_mut(&pk)
                    {
                        match pk_our_latest_vote.slot.cmp(&pk_their_latest_vote.slot) {
                            // Ordering::Less => If the other tree has seen a vote by this validator for a later slot than the current tree,
                            // then take their votes.

                            // Ordering::Equal => If both trees have seen votes from this validator for a slot `S`, they must
                            // be votes for different versions of slot `S`, so we need to update this tree with the updates
                            // from the other tree

                            // TODOs:
                            // 1) Ensure if the latest vote in our current tree is a duplicate, make sure we properly
                            // clean that up.
                            // 2) Ensure if their latest vote is a duplicate, we take all the duplicates from
                            // them, AND if the merge root is part of the duplicate ancestors, we'll have to add
                            // all the ancestors of the merge leaf
                            Ordering::Less | Ordering::Equal => {
                                Some(pk_their_latest_vote.into_votes(pk))
                            }

                            // If we have the bigger vote, we can ignore the other tree's latest vote for
                            // that validator
                            Ordering::Greater => None,
                        }
                    } else {
                        // If we don't have a vote for this key in the current tree,
                        // then take the other tree's votes
                        Some(pk_their_latest_vote.into_votes(pk))
                    }
                },
            )
            .flatten()
            .collect();

        // TODO: it's not safe to add votes for multiple versions of a slot per validator in same `add_votes()` batch
        self.add_votes(new_votes.iter(), epoch_stakes, epoch_schedule);
    }

    pub fn stake_voted_at(&self, slot: &SlotHashKey) -> Option<u64> {
        self.fork_infos
            .get(slot)
            .map(|fork_info| fork_info.stake_voted_at)
    }

    fn propagate_new_leaf(
        &mut self,
        slot_hash_key: &SlotHashKey,
        parent_slot_hash_key: &SlotHashKey,
    ) {
        let parent_best_slot_hash_key = self
            .best_slot(parent_slot_hash_key)
            .expect("parent must exist in self.fork_infos after its child leaf was created");

        // If this new leaf is the direct parent's best child, then propagate
        // it up the tree
        if self.is_best_child(slot_hash_key) {
            let mut ancestor = Some(*parent_slot_hash_key);
            loop {
                if ancestor.is_none() {
                    break;
                }
                let ancestor_fork_info = self.fork_infos.get_mut(&ancestor.unwrap()).unwrap();
                if ancestor_fork_info.best_slot == parent_best_slot_hash_key {
                    ancestor_fork_info.best_slot = *slot_hash_key;
                } else {
                    break;
                }
                ancestor = ancestor_fork_info.parent;
            }
        }
    }

    fn insert_mark_valid_aggregate_operations(
        &self,
        update_operations: &mut BTreeMap<(SlotHashKey, UpdateLabel), UpdateOperation>,
        slot_hash_key: SlotHashKey,
    ) {
        self.do_insert_aggregate_operations(update_operations, true, slot_hash_key);
    }

    fn insert_aggregate_operations(
        &self,
        update_operations: &mut BTreeMap<(SlotHashKey, UpdateLabel), UpdateOperation>,
        slot_hash_key: SlotHashKey,
    ) {
        self.do_insert_aggregate_operations(update_operations, false, slot_hash_key);
    }

    #[allow(clippy::map_entry)]
    fn do_insert_aggregate_operations(
        &self,
        update_operations: &mut BTreeMap<(SlotHashKey, UpdateLabel), UpdateOperation>,
        should_mark_valid: bool,
        slot_hash_key: SlotHashKey,
    ) {
        for parent_slot_hash_key in self.ancestor_iterator(slot_hash_key) {
            let aggregate_label = (parent_slot_hash_key, UpdateLabel::Aggregate);
            if update_operations.contains_key(&aggregate_label) {
                break;
            } else {
                if should_mark_valid {
                    update_operations.insert(
                        (parent_slot_hash_key, UpdateLabel::MarkValid),
                        UpdateOperation::MarkValid,
                    );
                }
                update_operations.insert(aggregate_label, UpdateOperation::Aggregate);
            }
        }
    }

    fn ancestor_iterator(&self, start_slot_hash_key: SlotHashKey) -> AncestorIterator {
        AncestorIterator::new(start_slot_hash_key, &self.fork_infos)
    }

    fn aggregate_slot(
        &mut self,
        slot_hash_key: SlotHashKey,
        duplicate_pubkeys_added: &[(Pubkey, u64)],
        duplicate_pubkeys_removed: &[Pubkey],
    ) {
        let mut stake_voted_subtree;
        let mut best_slot_hash_key = slot_hash_key;
        let mut duplicate_pubkeys_added_in_children: HashMap<Pubkey, u64> = HashMap::new();
        let mut duplicate_pubkey_stakes_found_in_children: HashMap<Pubkey, u64> = HashMap::new();
        if let Some(fork_info) = self.fork_infos.get(&slot_hash_key) {
            stake_voted_subtree = fork_info.stake_voted_at;
            let mut best_child_stake_voted_subtree = 0;
            let mut best_child_slot = slot_hash_key;
            for child in &fork_info.children {
                let child_fork_info = self.fork_infos.get(child).unwrap();
                let mut unique_child_stake_voted_subtree = child_fork_info.stake_voted_subtree;

                // TODO: This can later be optimized to track all the redundant stake and only update
                // on diffs, instead of scanning across every child's `duplicate_votes` every time
                for (child_duplicate_pubkey, child_duplicate_stake) in
                    child_fork_info.duplicate_votes.iter()
                {
                    let entry =
                        duplicate_pubkey_stakes_found_in_children.entry(*child_duplicate_pubkey);
                    match entry {
                        Entry::Occupied(occupied_entry) => {
                            // All previously found versions in other children better
                            // have the same stake
                            assert_eq!(*occupied_entry.get(), *child_duplicate_stake);
                            unique_child_stake_voted_subtree -= child_duplicate_stake;
                        }
                        Entry::Vacant(vacant_entry) => {
                            vacant_entry.insert(*child_duplicate_stake);
                        }
                    }
                }

                for (duplicate_pubkey_to_check, expected_pubkey_stake) in duplicate_pubkeys_added {
                    if let Some(pubkey_stake) =
                        child_fork_info.get_duplicate_pubkey_stake(&duplicate_pubkey_to_check)
                    {
                        // All previously found versions in other children better
                        // have the same stake
                        assert_eq!(pubkey_stake, *expected_pubkey_stake);

                        // Mark down this child has added this duplicate key, so we can later
                        // update our own fork info if we don't have this duplicate marked down
                        duplicate_pubkeys_added_in_children
                            .insert(*duplicate_pubkey_to_check, pubkey_stake);
                    }
                }

                // Child forks that are not candidates still contribute to the weight
                // of the subtree rooted at `slot_hash_key`. For instance:
                /*
                    Build fork structure:
                          slot 0
                            |
                          slot 1
                          /    \
                    slot 2     |
                        |     slot 3 (34%)
                slot 4 (66%)

                    If slot 4 is a duplicate slot, so no longer qualifies as a candidate until
                    the slot is confirmed, the weight of votes on slot 4 should still count towards
                    slot 2, otherwise we might pick slot 3 as the heaviest fork to build blocks on
                    instead of slot 2.
                */

                // See comment above for why this check is outside of the `is_candidate` check.
                stake_voted_subtree += unique_child_stake_voted_subtree;

                // Note: If there's no valid children, then the best slot should default to the
                // input `slot` itself.
                let child_stake_voted_subtree = child_fork_info.stake_voted_subtree;
                if child_fork_info.is_candidate
                    && (best_child_slot == slot_hash_key ||
                    child_stake_voted_subtree > best_child_stake_voted_subtree ||
                // tiebreaker by slot height, prioritize earlier slot
                (child_stake_voted_subtree == best_child_stake_voted_subtree && child < &best_child_slot))
                {
                    best_child_stake_voted_subtree = child_stake_voted_subtree;
                    best_child_slot = *child;
                    best_slot_hash_key = self
                        .best_slot(child)
                        .expect("`child` must exist in `self.fork_infos`");
                }
            }
        } else {
            return;
        }

        let fork_info = self.fork_infos.get_mut(&slot_hash_key).unwrap();
        fork_info.stake_voted_subtree = stake_voted_subtree;
        fork_info.best_slot = best_slot_hash_key;

        for (duplicate_pubkey, expected_stake) in duplicate_pubkeys_added_in_children.into_iter() {
            if let Some(prev_stake) = fork_info
                .duplicate_votes
                .insert(duplicate_pubkey, expected_stake)
            {
                assert_eq!(expected_stake, prev_stake);
            }
        }

        for removed_duplicate_pubkey in duplicate_pubkeys_removed {
            fork_info.duplicate_votes.remove(removed_duplicate_pubkey);
        }
    }

    fn mark_slot_valid(&mut self, valid_slot_hash_key: (Slot, Hash)) {
        if let Some(fork_info) = self.fork_infos.get_mut(&valid_slot_hash_key) {
            if !fork_info.is_candidate {
                info!(
                    "marked previously invalid fork starting at slot: {} as valid",
                    valid_slot
                );
            }
            fork_info.is_candidate = true;
        }
    }

    // Returns the previous duplicate entry
    fn insert_duplicate_vote(
        &mut self,
        slot_hash_key: &SlotHashKey,
        pubkey: Pubkey,
        stake: u64,
    ) -> Option<u64> {
        let fork_info = self
            .fork_infos
            .get_mut(slot_hash_key)
            .expect("latest voted block must exist in the tree");

        fork_info.duplicate_votes.insert(pubkey, stake)
    }

    fn generate_update_operations<'a, 'b>(
        &'a mut self,
        pubkey_votes: impl Iterator<Item = impl Borrow<(Pubkey, SlotHashKey)> + 'b>,
        epoch_stakes: &HashMap<Epoch, EpochStakes>,
        epoch_schedule: &EpochSchedule,
    ) -> (
        BTreeMap<(SlotHashKey, UpdateLabel), UpdateOperation>,
        Vec<(Pubkey, u64)>,
        Vec<Pubkey>,
    ) {
        let mut update_operations: BTreeMap<(SlotHashKey, UpdateLabel), UpdateOperation> =
            BTreeMap::new();
        // Validators in `pubkey_votes` that are voting on duplicate versions of a slot
        let mut duplicate_pubkeys_added = vec![];
        // Validators that voted on duplicate versions of a slot that have now been
        // outdated by a vote for a later slot
        let mut duplicate_pubkeys_removed = vec![];

        let mut observed_pubkeys = HashSet::new();
        // Sort the `pubkey_votes` in a BTreeMap by the slot voted
        for pubkey_vote in pubkey_votes {
            // A pubkey cannot appear in pubkey votes more than once, otherwise the logic
            // below for adding/subtracting stake is unsafe. Also if somebody adds a duplicate
            // vote for a different version of a slot, and simultaneously votes for a later slot
            // that outdates that duplicate vote, this would also break things.
            let (pubkey, new_vote_slot_hash) = pubkey_vote.borrow();
            if observed_pubkeys.contains(pubkey) {
                panic!("This update contained multiple updates for same validator key");
            } else {
                observed_pubkeys.insert(*pubkey);
            }

            let mut pubkey_latest_vote = self.latest_votes.get_mut(pubkey);

            // Filter out any votes or slots < any slot this pubkey has
            // already voted for, we only care about the latest votes.
            //
            // If the new vote is for the same slot, but a different hash,
            // allow processing to continue as this is a duplicate version
            // of the same slot.
            let new_vote_slot = new_vote_slot_hash.0;
            let new_vote_hash = new_vote_slot_hash.1;
            match pubkey_latest_vote
                .as_mut()
                .map(|latest_vote| (latest_vote.slot, &mut latest_vote.hashes))
            {
                Some((pubkey_latest_vote_slot, pubkey_latest_vote_hashes))
                    if (new_vote_slot == pubkey_latest_vote_slot
                        && !pubkey_latest_vote_hashes.contains(&new_vote_hash)) =>
                {
                    // Get the validator stake
                    let epoch = epoch_schedule.get_epoch(pubkey_latest_vote_slot);
                    let pubkey_stake = epoch_stakes
                        .get(&epoch)
                        .map(|epoch_stakes| epoch_stakes.vote_account_stake(pubkey))
                        .unwrap_or(0);

                    // If this is the first time we've seen this validator vote on
                    // a duplicate version of the same slot, then `first_duplicate_vote` is
                    // Some.
                    let first_duplicate_vote = if pubkey_latest_vote_hashes.len() == 1 {
                        Some((
                            pubkey_latest_vote_slot,
                            *pubkey_latest_vote_hashes.iter().next().unwrap(),
                        ))
                    } else {
                        None
                    };

                    // Push this new version into the list of voted hashes
                    pubkey_latest_vote_hashes.insert(new_vote_hash);

                    // Mark down that this validator committed a duplicate vote
                    duplicate_pubkeys_added.push((*pubkey, pubkey_stake));

                    // There must not have existed a previous duplicate vote for this pubkey,
                    // for this exact (slot, hash) because:
                    // 1) We know this is the first time this pubkey voted on this slot hash
                    // otherwise the `!pubkey_latest_vote_hashes.contains(&new_vote_hash))`
                    // check above would have failed. This means previous votes by this validator
                    // for other duplicate versions of this slot could not have flagged this block
                    // as duplicate, because no other duplicates can be on the same fork.
                    // 2) We know this is the *latest* slot due to the `new_vote_slot == pubkey_latest_vote_slot`
                    // check above, so we know no children of this slot can have a vote for this pubkey,
                    // so previous votes for children should not have set the duplicate flag
                    assert!(self
                        .insert_duplicate_vote(&new_vote_slot_hash, *pubkey, pubkey_stake)
                        .is_none());

                    // Check if have seen votes on multiple versions of a duplicate slot
                    // for this validator `pk` in this tree. If not, then this is the
                    // first time and we need to mark in all the ancestors of the latest vote
                    // that this validator is a duplicate
                    if let Some(first_duplicate_vote) = first_duplicate_vote {
                        // Mark `first_duplicate_vote` node in the tree with knowledge of this duplicate vote.
                        // There should not exist a previous entry in the `fork_info.duplicate_votes` for this pubkey
                        // since this is the first time we detected a duplicate vote for this pubkey, for this slot.
                        // Also note duplicate votes for previous slots would have already been cleared when this latest
                        // vote for `pubkey_latest_vote_slot` was first added.
                        assert!(self
                            .insert_duplicate_vote(&first_duplicate_vote, *pubkey, pubkey_stake)
                            .is_none());
                        // Need to mark all the ancestors of `first_duplicate_vote` as duplicate,
                        // so insert those ancestors into the aggregate operations
                        self.insert_aggregate_operations(
                            &mut update_operations,
                            first_duplicate_vote,
                        );
                    }
                }

                Some((pubkey_latest_vote_slot, pubkey_latest_vote_hashes))
                    if (new_vote_slot < pubkey_latest_vote_slot)
                        || (new_vote_slot == pubkey_latest_vote_slot
                            && pubkey_latest_vote_hashes.contains(&new_vote_slot_hash.1)) =>
                {
                    continue;
                }

                _ => {
                    // We either don't have a vote yet for this pubkey, or the new vote slot
                    // is bigger than the old vote slot. Remove this pubkey stake from previous fork of the previous vote
                    assert!(
                        pubkey_latest_vote.is_none() || {
                            if let Some(prev_latest_vote) = pubkey_latest_vote {
                                new_vote_slot > prev_latest_vote.slot
                            } else {
                                panic!("Must be Some")
                            }
                        }
                    );

                    let new_latest_vote = LatestVotes {
                        slot: new_vote_slot_hash.0,
                        hashes: vec![new_vote_slot_hash.1]
                            .into_iter()
                            .collect::<HashSet<Hash>>(),
                    };

                    if let Some(old_latest_vote) =
                        self.latest_votes.insert(*pubkey, new_latest_vote)
                    {
                        if old_latest_vote.is_duplicate() {
                            duplicate_pubkeys_removed.push(*pubkey);
                        }
                        let epoch = epoch_schedule.get_epoch(old_latest_vote.slot);
                        let stake_update = epoch_stakes
                            .get(&epoch)
                            .map(|epoch_stakes| epoch_stakes.vote_account_stake(pubkey))
                            .unwrap_or(0);

                        // TODO: this stake_update > 0 is not safe since we will have potentially
                        // addded duplicates with stake == 0 in the past
                        if stake_update > 0 {
                            for old_slot_hash in old_latest_vote.into_slot_hashes() {
                                update_operations
                                    .entry((old_slot_hash, UpdateLabel::Subtract))
                                    // Multiple *different* validators could have added/subtracted stake
                                    // from this specific block, but the *same* validator could have only done one update
                                    // (see usage of `observed_pubkeys` above), so we don't need to worry
                                    // about duplicates here
                                    .and_modify(|update| update.update_stake(stake_update))
                                    .or_insert(UpdateOperation::Subtract(stake_update));

                                // The parents of the old latest voted slot
                                // won't subtract this validator's stake multiple times, since the
                                // aggregate operation just sums the stakes of all the children,
                                // and the relevant children won't have any of the validator's stake
                                // counted toward them because the `Subtract` operations will delete
                                // all stakes for this validator's votes from all versions of the old
                                // voted slot
                                self.insert_aggregate_operations(
                                    &mut update_operations,
                                    old_slot_hash,
                                );
                            }
                        }
                    }
                }
            }

            // Add this pubkey stake to new fork
            let epoch = epoch_schedule.get_epoch(new_vote_slot_hash.0);
            let stake_update = epoch_stakes
                .get(&epoch)
                .map(|epoch_stakes| epoch_stakes.vote_account_stake(&pubkey))
                .unwrap_or(0);

            // TODO: handle if one version of the slot has more stake after crossing epoch
            // boundary, should always take the greater stake when propagating
            update_operations
                .entry((*new_vote_slot_hash, UpdateLabel::Add))
                // Multiple *different* validators could have added/subtracted stake
                // from this specific block, but the *same* validator could have only done one update
                // (see usage of `observed_pubkeys` above), so we don't need to worry
                // about duplicates here
                .and_modify(|update| update.update_stake(stake_update))
                .or_insert(UpdateOperation::Add(stake_update));
            self.insert_aggregate_operations(&mut update_operations, *new_vote_slot_hash);
        }

        (
            update_operations,
            duplicate_pubkeys_added,
            duplicate_pubkeys_removed,
        )
    }

    fn process_update_operations(
        &mut self,
        update_operations: BTreeMap<(SlotHashKey, UpdateLabel), UpdateOperation>,
        duplicate_pubkeys_added: &[(Pubkey, u64)],
        duplicate_pubkeys_removed: &[Pubkey],
    ) {
        // Iterate through the update operations from greatest to smallest slot
        for ((slot_hash_key, _), operation) in update_operations.into_iter().rev() {
            match operation {
                UpdateOperation::MarkValid => self.mark_slot_valid(slot_hash_key),
                UpdateOperation::Aggregate => self.aggregate_slot(
                    slot_hash_key,
                    duplicate_pubkeys_added,
                    duplicate_pubkeys_removed,
                ),
                UpdateOperation::Add(stake) => self.add_slot_stake(&slot_hash_key, stake),
                UpdateOperation::Subtract(stake) => self.subtract_slot_stake(&slot_hash_key, stake),
            }
        }
    }

    fn add_slot_stake(&mut self, slot_hash_key: &SlotHashKey, stake: u64) {
        if let Some(fork_info) = self.fork_infos.get_mut(slot_hash_key) {
            fork_info.stake_voted_at += stake;
            fork_info.stake_voted_subtree += stake;
        }
    }

    fn subtract_slot_stake(&mut self, slot_hash_key: &SlotHashKey, stake: u64) {
        if let Some(fork_info) = self.fork_infos.get_mut(slot_hash_key) {
            fork_info.stake_voted_at -= stake;
            fork_info.stake_voted_subtree -= stake;
        }
    }

    fn parent(&self, slot_hash_key: &SlotHashKey) -> Option<SlotHashKey> {
        self.fork_infos
            .get(slot_hash_key)
            .map(|fork_info| fork_info.parent)
            .unwrap_or(None)
    }

    fn print_state(&self) {
        let best_slot_hash_key = self.best_overall_slot();
        let mut best_path: VecDeque<_> = self.ancestor_iterator(best_slot_hash_key).collect();
        best_path.push_front(best_slot_hash_key);
        info!(
            "Latest known votes by vote pubkey: {:#?}, best path: {:?}",
            self.latest_votes,
            best_path.iter().rev().collect::<Vec<&SlotHashKey>>()
        );
    }

    fn heaviest_slot_on_same_voted_fork(&self, tower: &Tower) -> Option<SlotHashKey> {
        tower
            .last_voted_slot_hash()
            .map(|last_voted_slot_hash| {
                let heaviest_slot_hash_on_same_voted_fork = self.best_slot(&last_voted_slot_hash);
                if heaviest_slot_hash_on_same_voted_fork.is_none() {
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
                            "a bank at last_voted_slot({:?}) is a frozen bank so must have been \
                            added to heaviest_subtree_fork_choice at time of freezing",
                            last_voted_slot_hash,
                        )
                    } else {
                        // fork_infos doesn't have corresponding data for the stale stray last vote,
                        // meaning some inconsistency between saved tower and ledger.
                        // (newer snapshot, or only a saved tower is moved over to new setup?)
                        return None;
                    }
                }
                let heaviest_slot_hash_on_same_voted_fork =
                    heaviest_slot_hash_on_same_voted_fork.unwrap();

                if heaviest_slot_hash_on_same_voted_fork == last_voted_slot_hash {
                    None
                } else {
                    Some(heaviest_slot_hash_on_same_voted_fork)
                }
            })
            .unwrap_or(None)
    }

    #[cfg(test)]
    fn set_stake_voted_at(&mut self, slot_hash_key: SlotHashKey, stake_voted_at: u64) {
        self.fork_infos
            .get_mut(&slot_hash_key)
            .unwrap()
            .stake_voted_at = stake_voted_at;
    }

    #[cfg(test)]
    fn is_leaf(&self, slot_hash_key: SlotHashKey) -> bool {
        self.fork_infos
            .get(&slot_hash_key)
            .unwrap()
            .children
            .is_empty()
    }
}

impl TreeDiff for HeaviestSubtreeForkChoice {
    type TreeKey = SlotHashKey;
    fn contains_slot(&self, slot_hash_key: &SlotHashKey) -> bool {
        self.fork_infos.contains_key(slot_hash_key)
    }

    fn children(&self, slot_hash_key: &SlotHashKey) -> Option<&[SlotHashKey]> {
        self.fork_infos
            .get(&slot_hash_key)
            .map(|fork_info| &fork_info.children[..])
    }
}

impl ForkChoice for HeaviestSubtreeForkChoice {
    type ForkChoiceKey = SlotHashKey;
    fn compute_bank_stats(
        &mut self,
        bank: &Bank,
        _tower: &Tower,
        progress: &mut ProgressMap,
        computed_bank_state: &ComputedBankState,
    ) {
        let ComputedBankState { pubkey_votes, .. } = computed_bank_state;

        // Update `heaviest_subtree_fork_choice` to find the best fork to build on
        let root = self.root.0;
        let (best_overall_slot, best_overall_hash) = self.add_votes(
            pubkey_votes.iter().filter_map(|(pubkey, slot)| {
                if *slot >= root {
                    Some((
                        *pubkey,
                        (
                            *slot,
                            progress
                                .get_hash(*slot)
                                .expect("Votes for ancestors must exist in progress map"),
                        ),
                    ))
                } else {
                    None
                }
            }),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        datapoint_info!(
            "compute_bank_stats-best_slot",
            ("computed_slot", bank.slot(), i64),
            ("overall_best_slot", best_overall_slot, i64),
            ("overall_best_hash", best_overall_hash.to_string(), String),
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
            // BankForks should only contain one valid version of this slot
            r_bank_forks
                .get_with_checked_hash(self.best_overall_slot())
                .unwrap()
                .clone(),
            self.heaviest_slot_on_same_voted_fork(tower)
                .map(|slot_hash| {
                    // BankForks should only contain one valid version of this slot
                    r_bank_forks
                        .get_with_checked_hash(slot_hash)
                        .unwrap()
                        .clone()
                }),
        )
    }

    fn mark_fork_invalid_candidate(&mut self, invalid_slot_hash_key: &SlotHashKey) {
        info!(
            "marking fork starting at slot: {:?} invalid candidate",
            invalid_slot
        );
        let fork_info = self.fork_infos.get_mut(invalid_slot_hash_key);
        if let Some(fork_info) = fork_info {
            if fork_info.is_candidate {
                fork_info.is_candidate = false;
                // Aggregate to find the new best slots excluding this fork
                let mut aggregate_operations = BTreeMap::new();
                self.insert_aggregate_operations(&mut aggregate_operations, *invalid_slot_hash_key);
                self.process_update_operations(aggregate_operations, &[], &[]);
            }
        }
    }

    fn mark_fork_valid_candidate(&mut self, valid_slot_hash_key: &SlotHashKey) {
        let mut aggregate_operations = BTreeMap::new();
        let fork_info = self.fork_infos.get_mut(valid_slot_hash_key);
        if let Some(fork_info) = fork_info {
            // If a bunch of slots on the same fork are confirmed at once, then only the latest
            // slot will incur this aggregation operation
            fork_info.is_candidate = true;
            self.insert_mark_valid_aggregate_operations(
                &mut aggregate_operations,
                *valid_slot_hash_key,
            );
        }

        // Aggregate to find the new best slots including this fork
        self.process_update_operations(aggregate_operations, &[], &[]);
    }
}

struct AncestorIterator<'a> {
    current_slot_hash_key: SlotHashKey,
    fork_infos: &'a HashMap<SlotHashKey, ForkInfo>,
}

impl<'a> AncestorIterator<'a> {
    fn new(
        start_slot_hash_key: SlotHashKey,
        fork_infos: &'a HashMap<SlotHashKey, ForkInfo>,
    ) -> Self {
        Self {
            current_slot_hash_key: start_slot_hash_key,
            fork_infos,
        }
    }
}

impl<'a> Iterator for AncestorIterator<'a> {
    type Item = (Slot, Hash);
    fn next(&mut self) -> Option<Self::Item> {
        let parent_slot_hash_key = self
            .fork_infos
            .get(&self.current_slot_hash_key)
            .map(|fork_info| fork_info.parent)
            .unwrap_or(None);

        parent_slot_hash_key
            .map(|parent_slot_hash_key| {
                self.current_slot_hash_key = parent_slot_hash_key;
                Some(self.current_slot_hash_key)
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
            [(vote_pubkeys[0], (4, Hash::default()))].iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        assert_eq!(
            heaviest_subtree_fork_choice.max_by_weight((4, Hash::default()), (5, Hash::default())),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            heaviest_subtree_fork_choice.max_by_weight((4, Hash::default()), (0, Hash::default())),
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
            [(vote_pubkeys[0], (5, Hash::default()))].iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        heaviest_subtree_fork_choice.add_root_parent((2, Hash::default()));
        assert_eq!(
            heaviest_subtree_fork_choice
                .parent(&(3, Hash::default()))
                .unwrap(),
            (2, Hash::default())
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_subtree(&(3, Hash::default()))
                .unwrap(),
            stake
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&(2, Hash::default()))
                .unwrap(),
            0
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .children(&(2, Hash::default()))
                .unwrap()
                .to_vec(),
            vec![(3, Hash::default())]
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .best_slot(&(2, Hash::default()))
                .unwrap()
                .0,
            5
        );
        assert!(heaviest_subtree_fork_choice
            .parent(&(2, Hash::default()))
            .is_none());
    }

    #[test]
    fn test_ancestor_iterator() {
        let mut heaviest_subtree_fork_choice = setup_forks();
        let parents: Vec<_> = heaviest_subtree_fork_choice
            .ancestor_iterator((6, Hash::default()))
            .collect();
        assert_eq!(
            parents,
            vec![5, 3, 1, 0]
                .into_iter()
                .map(|s| (s, Hash::default()))
                .collect::<Vec<_>>()
        );
        let parents: Vec<_> = heaviest_subtree_fork_choice
            .ancestor_iterator((4, Hash::default()))
            .collect();
        assert_eq!(
            parents,
            vec![2, 1, 0]
                .into_iter()
                .map(|s| (s, Hash::default()))
                .collect::<Vec<_>>()
        );
        let parents: Vec<_> = heaviest_subtree_fork_choice
            .ancestor_iterator((1, Hash::default()))
            .collect();
        assert_eq!(
            parents,
            vec![0]
                .into_iter()
                .map(|s| (s, Hash::default()))
                .collect::<Vec<_>>()
        );
        let parents: Vec<_> = heaviest_subtree_fork_choice
            .ancestor_iterator((0, Hash::default()))
            .collect();
        assert!(parents.is_empty());

        // Set a root, everything but slots 2, 4 should be removed
        heaviest_subtree_fork_choice.set_root((2, Hash::default()));
        let parents: Vec<_> = heaviest_subtree_fork_choice
            .ancestor_iterator((4, Hash::default()))
            .collect();
        assert_eq!(
            parents,
            vec![2]
                .into_iter()
                .map(|s| (s, Hash::default()))
                .collect::<Vec<_>>()
        );
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

        let root_bank = bank_forks.read().unwrap().root_bank();
        let root = root_bank.slot();
        let root_hash = root_bank.hash();
        let heaviest_subtree_fork_choice =
            HeaviestSubtreeForkChoice::new_from_frozen_banks((root, root_hash), &frozen_banks);

        let bank0_hash = bank_forks.read().unwrap().get(0).unwrap().hash();
        assert!(heaviest_subtree_fork_choice
            .parent(&(0, bank0_hash))
            .is_none());

        let bank1_hash = bank_forks.read().unwrap().get(1).unwrap().hash();
        assert_eq!(
            heaviest_subtree_fork_choice
                .children(&(0, bank0_hash))
                .unwrap(),
            &[(1, bank1_hash)]
        );

        assert_eq!(
            heaviest_subtree_fork_choice.parent(&(1, bank1_hash)),
            Some((0, bank0_hash))
        );
        let bank2_hash = bank_forks.read().unwrap().get(2).unwrap().hash();
        let bank3_hash = bank_forks.read().unwrap().get(3).unwrap().hash();
        assert_eq!(
            heaviest_subtree_fork_choice
                .children(&(1, bank1_hash))
                .unwrap(),
            &[(2, bank2_hash), (3, bank3_hash)]
        );
        assert_eq!(
            heaviest_subtree_fork_choice.parent(&(2, bank2_hash)),
            Some((1, bank1_hash))
        );
        let bank4_hash = bank_forks.read().unwrap().get(4).unwrap().hash();
        assert_eq!(
            heaviest_subtree_fork_choice
                .children(&(2, bank2_hash))
                .unwrap(),
            &[(4, bank4_hash)]
        );
        assert_eq!(
            heaviest_subtree_fork_choice.parent(&(3, bank3_hash)),
            Some((1, bank1_hash))
        );
        assert!(heaviest_subtree_fork_choice
            .children(&(3, bank3_hash))
            .unwrap()
            .is_empty());
        assert_eq!(
            heaviest_subtree_fork_choice.parent(&(4, bank4_hash)),
            Some((2, bank2_hash))
        );
        assert!(heaviest_subtree_fork_choice
            .children(&(4, bank4_hash))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_set_root() {
        let mut heaviest_subtree_fork_choice = setup_forks();

        // Set root to 1, should only purge 0
        heaviest_subtree_fork_choice.set_root((1, Hash::default()));
        for i in 0..=6 {
            let exists = i != 0;
            assert_eq!(
                heaviest_subtree_fork_choice
                    .fork_infos
                    .contains_key(&(i, Hash::default())),
                exists
            );
        }

        // Set root to 5, should purge everything except 5, 6
        heaviest_subtree_fork_choice.set_root((5, Hash::default()));
        for i in 0..=6 {
            let exists = i == 5 || i == 6;
            assert_eq!(
                heaviest_subtree_fork_choice
                    .fork_infos
                    .contains_key(&(i, Hash::default())),
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
            [(vote_pubkeys[0], (1, Hash::default()))].iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 4);

        // Set a root
        heaviest_subtree_fork_choice.set_root((1, Hash::default()));

        // Vote again for slot 3 on a different fork than the last vote,
        // verify this fork is now the best fork
        heaviest_subtree_fork_choice.add_votes(
            [(vote_pubkeys[0], (3, Hash::default()))].iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 6);
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&(1, Hash::default()))
                .unwrap(),
            0
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&(3, Hash::default()))
                .unwrap(),
            stake
        );
        for slot in &[1, 3] {
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(&(*slot, Hash::default()))
                    .unwrap(),
                stake
            );
        }

        // Set a root at last vote
        heaviest_subtree_fork_choice.set_root((3, Hash::default()));
        // Check new leaf 7 is still propagated properly
        heaviest_subtree_fork_choice
            .add_new_leaf_slot((7, Hash::default()), Some((6, Hash::default())));
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 7);
    }

    #[test]
    fn test_set_root_and_add_outdated_votes() {
        let mut heaviest_subtree_fork_choice = setup_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(1, stake);

        // Vote for slot 0
        heaviest_subtree_fork_choice.add_votes(
            [(vote_pubkeys[0], (0, Hash::default()))].iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Set root to 1, should purge 0 from the tree, but
        // there's still an outstanding vote for slot 0 in `pubkey_votes`.
        heaviest_subtree_fork_choice.set_root((1, Hash::default()));

        // Vote again for slot 3, verify everything is ok
        heaviest_subtree_fork_choice.add_votes(
            [(vote_pubkeys[0], (3, Hash::default()))].iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&(3, Hash::default()))
                .unwrap(),
            stake
        );
        for slot in &[1, 3] {
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(&(*slot, Hash::default()))
                    .unwrap(),
                stake
            );
        }
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 6);

        // Set root again on different fork than the last vote
        heaviest_subtree_fork_choice.set_root((2, Hash::default()));
        // Smaller vote than last vote 3 should be ignored
        heaviest_subtree_fork_choice.add_votes(
            [(vote_pubkeys[0], (2, Hash::default()))].iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&(2, Hash::default()))
                .unwrap(),
            0
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_subtree(&(2, Hash::default()))
                .unwrap(),
            0
        );
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 4);

        // New larger vote than last vote 3 should be processed
        heaviest_subtree_fork_choice.add_votes(
            [(vote_pubkeys[0], (4, Hash::default()))].iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&(2, Hash::default()))
                .unwrap(),
            0
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&(4, Hash::default()))
                .unwrap(),
            stake
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_subtree(&(2, Hash::default()))
                .unwrap(),
            stake
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_subtree(&(4, Hash::default()))
                .unwrap(),
            stake
        );
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 4);
    }

    #[test]
    fn test_best_overall_slot() {
        let heaviest_subtree_fork_choice = setup_forks();
        // Best overall path is 0 -> 1 -> 2 -> 4, so best leaf
        // should be 4
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 4);
    }

    #[test]
    fn test_add_new_leaf_duplicate() {
        let (
            mut heaviest_subtree_fork_choice,
            duplicate_leaves_descended_from_4,
            duplicate_leaves_descended_from_5,
        ) = setup_duplicate_forks();

        // Add a child to one of the duplicates
        let duplicate_parent = duplicate_leaves_descended_from_4[0];
        let child = (11, Hash::new_unique());
        heaviest_subtree_fork_choice.add_new_leaf_slot(child, Some(duplicate_parent));
        assert_eq!(
            heaviest_subtree_fork_choice
                .children(&duplicate_parent)
                .unwrap(),
            &[child]
        );
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), child);

        // All the other duplicates should have no children
        for duplicate_leaf in duplicate_leaves_descended_from_5
            .iter()
            .chain(std::iter::once(&duplicate_leaves_descended_from_4[1]))
        {
            assert!(heaviest_subtree_fork_choice
                .children(&duplicate_leaf)
                .unwrap()
                .is_empty(),);
        }

        // Re-adding same duplicate slot should not overwrite existing one
        heaviest_subtree_fork_choice
            .add_new_leaf_slot(duplicate_parent, Some((4, Hash::default())));
        assert_eq!(
            heaviest_subtree_fork_choice
                .children(&duplicate_parent)
                .unwrap(),
            &[child]
        );
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot(), child);
    }

    #[test]
    fn test_propagate_new_leaf() {
        let mut heaviest_subtree_fork_choice = setup_forks();

        // Add a leaf 10, it should be the best choice
        heaviest_subtree_fork_choice
            .add_new_leaf_slot((10, Hash::default()), Some((4, Hash::default())));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator((10, Hash::default()))
            .collect::<Vec<_>>();
        for a in ancestors
            .into_iter()
            .chain(std::iter::once((10, Hash::default())))
        {
            assert_eq!(heaviest_subtree_fork_choice.best_slot(&a).unwrap().0, 10);
        }

        // Add a smaller leaf 9, it should be the best choice
        heaviest_subtree_fork_choice
            .add_new_leaf_slot((9, Hash::default()), Some((4, Hash::default())));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator((9, Hash::default()))
            .collect::<Vec<_>>();
        for a in ancestors
            .into_iter()
            .chain(std::iter::once((9, Hash::default())))
        {
            assert_eq!(heaviest_subtree_fork_choice.best_slot(&a).unwrap().0, 9);
        }

        // Add a higher leaf 11, should not change the best choice
        heaviest_subtree_fork_choice
            .add_new_leaf_slot((11, Hash::default()), Some((4, Hash::default())));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator((11, Hash::default()))
            .collect::<Vec<_>>();
        for a in ancestors
            .into_iter()
            .chain(std::iter::once((9, Hash::default())))
        {
            assert_eq!(heaviest_subtree_fork_choice.best_slot(&a).unwrap().0, 9);
        }

        // Add a vote for the other branch at slot 3.
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(2, stake);
        let leaf6 = 6;
        // Leaf slot 9 stops being the `best_slot` at slot 1 because there
        // are now votes for the branch at slot 3
        heaviest_subtree_fork_choice.add_votes(
            [(vote_pubkeys[0], (leaf6, Hash::default()))].iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Because slot 1 now sees the child branch at slot 3 has non-zero
        // weight, adding smaller leaf slot 8 in the other child branch at slot 2
        // should not propagate past slot 1
        heaviest_subtree_fork_choice
            .add_new_leaf_slot((8, Hash::default()), Some((4, Hash::default())));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator((8, Hash::default()))
            .collect::<Vec<_>>();
        for a in ancestors
            .into_iter()
            .chain(std::iter::once((8, Hash::default())))
        {
            let best_slot = if a.0 > 1 { 8 } else { leaf6 };
            assert_eq!(
                heaviest_subtree_fork_choice.best_slot(&a).unwrap().0,
                best_slot
            );
        }

        // Add vote for slot 8, should now be the best slot (has same weight
        // as fork containing slot 6, but slot 2 is smaller than slot 3).
        heaviest_subtree_fork_choice.add_votes(
            [(vote_pubkeys[1], (8, Hash::default()))].iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 8);

        // Because slot 4 now sees the child leaf 8 has non-zero
        // weight, adding smaller leaf slots should not propagate past slot 4
        heaviest_subtree_fork_choice
            .add_new_leaf_slot((7, Hash::default()), Some((4, Hash::default())));
        let ancestors = heaviest_subtree_fork_choice
            .ancestor_iterator((7, Hash::default()))
            .collect::<Vec<_>>();
        for a in ancestors
            .into_iter()
            .chain(std::iter::once((8, Hash::default())))
        {
            assert_eq!(heaviest_subtree_fork_choice.best_slot(&a).unwrap().0, 8);
        }

        // All the leaves should think they are their own best choice
        for leaf in [8, 9, 10, 11].iter() {
            assert_eq!(
                heaviest_subtree_fork_choice
                    .best_slot(&(*leaf, Hash::default()))
                    .unwrap()
                    .0,
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
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 6);

        // Add a leaf slot 5. Even though 5 is less than the best leaf 6,
        // it's not less than it's sibling slot 4, so the best overall
        // leaf should remain unchanged
        heaviest_subtree_fork_choice
            .add_new_leaf_slot((5, Hash::default()), Some((0, Hash::default())));
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 6);

        // Add a leaf slot 2 on a different fork than leaf 6. Slot 2 should
        // be the new best because it's for a lesser slot
        heaviest_subtree_fork_choice
            .add_new_leaf_slot((2, Hash::default()), Some((0, Hash::default())));
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 2);

        // Add a vote for slot 4, so leaf 6 should be the best again
        heaviest_subtree_fork_choice.add_votes(
            [(vote_pubkeys[0], (4, Hash::default()))].iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 6);

        // Adding a slot 1 that is less than the current best leaf 6 should not change the best
        // slot because the fork slot 5 is on has a higher weight
        heaviest_subtree_fork_choice
            .add_new_leaf_slot((1, Hash::default()), Some((0, Hash::default())));
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 6);
    }

    #[test]
    fn test_aggregate_slot() {
        let mut heaviest_subtree_fork_choice = setup_forks();

        // No weights are present, weights should be zero
        heaviest_subtree_fork_choice.aggregate_slot((1, Hash::default()), &[], &[]);
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&(1, Hash::default()))
                .unwrap(),
            0
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_subtree(&(1, Hash::default()))
                .unwrap(),
            0
        );
        // The best leaf when weights are equal should prioritize the lower leaf
        assert_eq!(
            heaviest_subtree_fork_choice
                .best_slot(&(1, Hash::default()))
                .unwrap()
                .0,
            4
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .best_slot(&(2, Hash::default()))
                .unwrap()
                .0,
            4
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .best_slot(&(3, Hash::default()))
                .unwrap()
                .0,
            6
        );

        // Update the weights that have voted *exactly* at each slot, the
        // branch containing slots {5, 6} has weight 11, so should be heavier
        // than the branch containing slots {2, 4}
        let mut total_stake = 0;
        let staked_voted_slots: HashSet<_> = vec![2, 4, 5, 6].into_iter().collect();
        for slot in &staked_voted_slots {
            heaviest_subtree_fork_choice.set_stake_voted_at((*slot, Hash::default()), *slot);
            total_stake += *slot;
        }

        // Aggregate up each of the two forks (order matters, has to be
        // reverse order for each fork, and aggregating a slot multiple times
        // is fine)
        let slots_to_aggregate: Vec<_> = std::iter::once((6, Hash::default()))
            .chain(heaviest_subtree_fork_choice.ancestor_iterator((6, Hash::default())))
            .chain(std::iter::once((4, Hash::default())))
            .chain(heaviest_subtree_fork_choice.ancestor_iterator((4, Hash::default())))
            .collect();

        for slot_hash in slots_to_aggregate {
            heaviest_subtree_fork_choice.aggregate_slot(slot_hash, &[], &[]);
        }

        // The best path is now 0 -> 1 -> 3 -> 5 -> 6, so leaf 6
        // should be the best choice
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 6);

        // Verify `stake_voted_at`
        for slot in 0..=6 {
            let expected_stake = if staked_voted_slots.contains(&slot) {
                slot
            } else {
                0
            };

            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_at(&(slot, Hash::default()))
                    .unwrap(),
                expected_stake
            );
        }

        // Verify `stake_voted_subtree` for common fork
        for slot in &[0, 1] {
            // Subtree stake is sum of the `stake_voted_at` across
            // all slots in the subtree
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(&(*slot, Hash::default()))
                    .unwrap(),
                total_stake
            );
        }
        // Verify `stake_voted_subtree` for fork 1
        let mut total_expected_stake = 0;
        for slot in &[4, 2] {
            total_expected_stake += heaviest_subtree_fork_choice
                .stake_voted_at(&(*slot, Hash::default()))
                .unwrap();
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(&(*slot, Hash::default()))
                    .unwrap(),
                total_expected_stake
            );
        }
        // Verify `stake_voted_subtree` for fork 2
        total_expected_stake = 0;
        for slot in &[6, 5, 3] {
            total_expected_stake += heaviest_subtree_fork_choice
                .stake_voted_at(&(*slot, Hash::default()))
                .unwrap();
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(&(*slot, Hash::default()))
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

        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[0], (3, Hash::default())),
            (vote_pubkeys[1], (2, Hash::default())),
            (vote_pubkeys[2], (1, Hash::default())),
        ];
        let expected_best_slot =
            |slot, heaviest_subtree_fork_choice: &HeaviestSubtreeForkChoice| -> Slot {
                if !heaviest_subtree_fork_choice.is_leaf((slot, Hash::default())) {
                    // Both branches have equal weight, so should pick the lesser leaf
                    if heaviest_subtree_fork_choice
                        .ancestor_iterator((4, Hash::default()))
                        .collect::<HashSet<SlotHashKey>>()
                        .contains(&(slot, Hash::default()))
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
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[0], (4, Hash::default())),
            (vote_pubkeys[1], (3, Hash::default())),
            (vote_pubkeys[2], (3, Hash::default())),
        ];

        let expected_best_slot =
            |slot, heaviest_subtree_fork_choice: &HeaviestSubtreeForkChoice| -> Slot {
                if !heaviest_subtree_fork_choice.is_leaf((slot, Hash::default())) {
                    // The branch with leaf 6 now has two votes, so should pick that one
                    if heaviest_subtree_fork_choice
                        .ancestor_iterator((6, Hash::default()))
                        .collect::<HashSet<SlotHashKey>>()
                        .contains(&(slot, Hash::default()))
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
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[0], (3, Hash::default())),
            (vote_pubkeys[1], (4, Hash::default())),
            (vote_pubkeys[2], (1, Hash::default())),
        ];

        let expected_update_operations: BTreeMap<(SlotHashKey, UpdateLabel), UpdateOperation> =
            vec![
                // Add/remove from new/old forks
                (
                    ((1, Hash::default()), UpdateLabel::Add),
                    UpdateOperation::Add(stake),
                ),
                (
                    ((3, Hash::default()), UpdateLabel::Add),
                    UpdateOperation::Add(stake),
                ),
                (
                    ((4, Hash::default()), UpdateLabel::Add),
                    UpdateOperation::Add(stake),
                ),
                // Aggregate all ancestors of changed slots
                (
                    ((0, Hash::default()), UpdateLabel::Aggregate),
                    UpdateOperation::Aggregate,
                ),
                (
                    ((1, Hash::default()), UpdateLabel::Aggregate),
                    UpdateOperation::Aggregate,
                ),
                (
                    ((2, Hash::default()), UpdateLabel::Aggregate),
                    UpdateOperation::Aggregate,
                ),
            ]
            .into_iter()
            .collect();

        let generated_update_operations = heaviest_subtree_fork_choice.generate_update_operations(
            pubkey_votes.iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(
            (expected_update_operations, vec![], vec![]),
            generated_update_operations
        );

        // Everyone makes older/same votes, should be ignored
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[0], (3, Hash::default())),
            (vote_pubkeys[1], (2, Hash::default())),
            (vote_pubkeys[2], (1, Hash::default())),
        ];
        let generated_update_operations = heaviest_subtree_fork_choice.generate_update_operations(
            pubkey_votes.iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert!(generated_update_operations.0.is_empty());

        // Some people make newer votes
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            // old, ignored
            (vote_pubkeys[0], (3, Hash::default())),
            // new, switched forks
            (vote_pubkeys[1], (5, Hash::default())),
            // new, same fork
            (vote_pubkeys[2], (3, Hash::default())),
        ];

        let expected_update_operations: BTreeMap<(SlotHashKey, UpdateLabel), UpdateOperation> =
            vec![
                // Add/remove to/from new/old forks
                (
                    ((3, Hash::default()), UpdateLabel::Add),
                    UpdateOperation::Add(stake),
                ),
                (
                    ((5, Hash::default()), UpdateLabel::Add),
                    UpdateOperation::Add(stake),
                ),
                (
                    ((1, Hash::default()), UpdateLabel::Subtract),
                    UpdateOperation::Subtract(stake),
                ),
                (
                    ((4, Hash::default()), UpdateLabel::Subtract),
                    UpdateOperation::Subtract(stake),
                ),
                // Aggregate all ancestors of changed slots
                (
                    ((0, Hash::default()), UpdateLabel::Aggregate),
                    UpdateOperation::Aggregate,
                ),
                (
                    ((1, Hash::default()), UpdateLabel::Aggregate),
                    UpdateOperation::Aggregate,
                ),
                (
                    ((2, Hash::default()), UpdateLabel::Aggregate),
                    UpdateOperation::Aggregate,
                ),
                (
                    ((3, Hash::default()), UpdateLabel::Aggregate),
                    UpdateOperation::Aggregate,
                ),
            ]
            .into_iter()
            .collect();

        let generated_update_operations = heaviest_subtree_fork_choice.generate_update_operations(
            pubkey_votes.iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(
            (expected_update_operations, vec![], vec![]),
            generated_update_operations
        );

        // People make new votes
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            // new, switch forks
            (vote_pubkeys[0], (4, Hash::default())),
            // new, same fork
            (vote_pubkeys[1], (6, Hash::default())),
            // new, same fork
            (vote_pubkeys[2], (6, Hash::default())),
        ];

        let expected_update_operations: BTreeMap<(SlotHashKey, UpdateLabel), UpdateOperation> =
            vec![
                // Add/remove from new/old forks
                (
                    ((4, Hash::default()), UpdateLabel::Add),
                    UpdateOperation::Add(stake),
                ),
                (
                    ((6, Hash::default()), UpdateLabel::Add),
                    UpdateOperation::Add(2 * stake),
                ),
                (
                    ((3, Hash::default()), UpdateLabel::Subtract),
                    UpdateOperation::Subtract(2 * stake),
                ),
                (
                    ((5, Hash::default()), UpdateLabel::Subtract),
                    UpdateOperation::Subtract(stake),
                ),
                // Aggregate all ancestors of changed slots
                (
                    ((0, Hash::default()), UpdateLabel::Aggregate),
                    UpdateOperation::Aggregate,
                ),
                (
                    ((1, Hash::default()), UpdateLabel::Aggregate),
                    UpdateOperation::Aggregate,
                ),
                (
                    ((2, Hash::default()), UpdateLabel::Aggregate),
                    UpdateOperation::Aggregate,
                ),
                (
                    ((3, Hash::default()), UpdateLabel::Aggregate),
                    UpdateOperation::Aggregate,
                ),
                (
                    ((5, Hash::default()), UpdateLabel::Aggregate),
                    UpdateOperation::Aggregate,
                ),
            ]
            .into_iter()
            .collect();

        let generated_update_operations = heaviest_subtree_fork_choice.generate_update_operations(
            pubkey_votes.iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(
            (expected_update_operations, vec![], vec![]),
            generated_update_operations
        );
    }

    #[test]
    fn test_add_votes() {
        let mut heaviest_subtree_fork_choice = setup_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(3, stake);

        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[0], (3, Hash::default())),
            (vote_pubkeys[1], (2, Hash::default())),
            (vote_pubkeys[2], (1, Hash::default())),
        ];
        assert_eq!(
            heaviest_subtree_fork_choice
                .add_votes(
                    pubkey_votes.iter(),
                    bank.epoch_stakes_map(),
                    bank.epoch_schedule()
                )
                .0,
            4
        );

        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 4)
    }

    #[test]
    fn test_add_votes_duplicate() {
        let (
            mut heaviest_subtree_fork_choice,
            duplicate_leaves_descended_from_4,
            duplicate_leaves_descended_from_5,
        ) = setup_duplicate_forks();
        let stake = 100;
        let num_validators = 5;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(num_validators, stake);

        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[0], duplicate_leaves_descended_from_4[0]),
            (vote_pubkeys[1], duplicate_leaves_descended_from_4[1]),
        ];

        // duplicate_leaves_descended_from_4 are sorted, and fork choice will pick the smaller
        // one in the event of a tie
        let expected_best_slot_hash = duplicate_leaves_descended_from_4[0];
        assert_eq!(
            heaviest_subtree_fork_choice.add_votes(
                pubkey_votes.iter(),
                bank.epoch_stakes_map(),
                bank.epoch_schedule()
            ),
            expected_best_slot_hash
        );

        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            expected_best_slot_hash
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&duplicate_leaves_descended_from_4[1])
                .unwrap(),
            stake
        );

        // Adding the same vote again will not do anything
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> =
            vec![(vote_pubkeys[1], duplicate_leaves_descended_from_4[1])];
        assert_eq!(
            heaviest_subtree_fork_choice.add_votes(
                pubkey_votes.iter(),
                bank.epoch_stakes_map(),
                bank.epoch_schedule()
            ),
            expected_best_slot_hash
        );
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            expected_best_slot_hash
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&duplicate_leaves_descended_from_4[1])
                .unwrap(),
            stake
        );

        // All the common ancestors of both duplicate leaves should have 2 * stake
        let expected_ancestors_stake = 2 * stake;
        for ancestor in
            heaviest_subtree_fork_choice.ancestor_iterator(duplicate_leaves_descended_from_4[1])
        {
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(&ancestor)
                    .unwrap(),
                expected_ancestors_stake
            );
        }

        // Adding a duplicate vote for a validator, for another version of a slot it has
        // already voted for
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> =
            vec![(vote_pubkeys[0], duplicate_leaves_descended_from_4[1])];

        // Now we have a duplicate vote on the other duplicate, it's now the heaviest branch,
        // as it has two votes.
        let expected_best_slot_hash = duplicate_leaves_descended_from_4[1];
        assert_eq!(
            heaviest_subtree_fork_choice.add_votes(
                pubkey_votes.iter(),
                bank.epoch_stakes_map(),
                bank.epoch_schedule()
            ),
            expected_best_slot_hash
        );

        // The leaf should now have two votes
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_subtree(&duplicate_leaves_descended_from_4[1])
                .unwrap(),
            2 * stake
        );

        // Common ancestors should not double count the vote, so those weights should remain unchanged.
        for ancestor in
            heaviest_subtree_fork_choice.ancestor_iterator(duplicate_leaves_descended_from_4[1])
        {
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(&ancestor)
                    .unwrap(),
                expected_ancestors_stake
            )
        }

        // Add some other votes for branch 5, should now become the heaviest branch
        // as it has 3 votes
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[2], duplicate_leaves_descended_from_5[0]),
            (vote_pubkeys[3], duplicate_leaves_descended_from_5[0]),
            (vote_pubkeys[4], (5, Hash::default())),
        ];
        let expected_best_slot_hash = duplicate_leaves_descended_from_5[0];
        assert_eq!(
            heaviest_subtree_fork_choice.add_votes(
                pubkey_votes.iter(),
                bank.epoch_stakes_map(),
                bank.epoch_schedule()
            ),
            expected_best_slot_hash
        );

        // Slots 0, 1 should have weight exactly `num_validators * stake`, since every
        // validator has voted
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_subtree(&(0, Hash::default()))
                .unwrap(),
            num_validators as u64 * stake
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_subtree(&(1, Hash::default()))
                .unwrap(),
            num_validators as u64 * stake
        );

        // Slots 2, 4 should have weight exactly `2 * stake`, since the first two
        // validators voted on that branch, even though it was duplicates
        for slot in &[2, 4] {
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(&(*slot, Hash::default()))
                    .unwrap(),
                2 * stake as u64
            );
        }

        // Slots 3, 5 should have weight exactly `3 * stake`, since the latter three
        // validators voted on that branch
        for slot in &[3, 5] {
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(&(*slot, Hash::default()))
                    .unwrap(),
                3 * stake as u64
            );
        }

        // Add more duplicate votes back to slot 4, branch 4 now should have 4 validators
        // voting on it with weight `4 * stake`, so it has become the heaviest again
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[2], duplicate_leaves_descended_from_4[0]),
            (vote_pubkeys[3], duplicate_leaves_descended_from_4[0]),
        ];
        let expected_best_slot_hash = duplicate_leaves_descended_from_4[0];
        assert_eq!(
            heaviest_subtree_fork_choice.add_votes(
                pubkey_votes.iter(),
                bank.epoch_stakes_map(),
                bank.epoch_schedule()
            ),
            expected_best_slot_hash
        );

        // Slots 0, 1 should have weight exactly `num_validators * stake`, since every
        // validator has voted
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_subtree(&(0, Hash::default()))
                .unwrap(),
            num_validators as u64 * stake
        );
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_subtree(&(1, Hash::default()))
                .unwrap(),
            num_validators as u64 * stake
        );

        // Slots 2, 4 should have weight exactly `4 * stake`, since 4 validators have double
        // voted on that branch
        for slot in &[2, 4] {
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(&(*slot, Hash::default()))
                    .unwrap(),
                4 * stake
            );
        }

        // Slots 3, 5 should have weight exactly `3 * stake`, since the latter three
        // validators voted on that branch
        for slot in &[3, 5] {
            assert_eq!(
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(&(*slot, Hash::default()))
                    .unwrap(),
                3 * stake
            );
        }

        // Add a duplicate vote for a slot that isn't a leaf, i.e. an inner tree node

        // Create two children for slots greater than the duplicate slot,
        // 1) descended from the current best slot (which also happens to be a duplicate slot)
        // 2) another descended from a non-duplicate slot.
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            duplicate_leaves_descended_from_4[0]
        );
        // Create new child with heaviest duplicate parent
        let duplicate_parent = duplicate_leaves_descended_from_4[0];
        let duplicate_slot = duplicate_parent.0;

        // Create new child with non-duplicate parent
        let nonduplicate_parent = (2, Hash::default());
        let higher_child_with_duplicate_parent = (duplicate_slot + 1, Hash::new_unique());
        let higher_child_with_nonduplicate_parent = (duplicate_slot + 2, Hash::new_unique());
        heaviest_subtree_fork_choice
            .add_new_leaf_slot(higher_child_with_duplicate_parent, Some(duplicate_parent));
        heaviest_subtree_fork_choice.add_new_leaf_slot(
            higher_child_with_nonduplicate_parent,
            Some(nonduplicate_parent),
        );

        // TODO: Vote for a higher leaf with the same pubkeys as the earlier votes,
        // will remove the stake voted at a duplicate slot

        // vote_pubkeys[0] and vote_pubkeys[2] have voted on two versions of the duplicate slot, both
        // should be erased after a vote for the higher parent
        let mut voted_slot_hashes = heaviest_subtree_fork_choice
            .latest_voted_slot_hashes(&vote_pubkeys[0])
            .unwrap();
        voted_slot_hashes.sort();
        let mut expected_slot_hashes = duplicate_leaves_descended_from_4.clone();
        expected_slot_hashes.sort();
        assert_eq!(voted_slot_hashes, expected_slot_hashes);

        let mut voted_slot_hashes = heaviest_subtree_fork_choice
            .latest_voted_slot_hashes(&vote_pubkeys[2])
            .unwrap();
        voted_slot_hashes.sort();
        let mut expected_slot_hashes = vec![
            duplicate_leaves_descended_from_4[0],
            duplicate_leaves_descended_from_5[0],
        ];
        expected_slot_hashes.sort();
        assert_eq!(voted_slot_hashes, expected_slot_hashes);

        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[0], higher_child_with_duplicate_parent),
            (vote_pubkeys[2], higher_child_with_duplicate_parent),
            (vote_pubkeys[1], higher_child_with_nonduplicate_parent),
        ];
        // Weight should be the three validators that voted on this slot
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&duplicate_leaves_descended_from_4[0])
                .unwrap(),
            3 * stake,
        );
        // Weight should be 2 * stake since, both vote_pubkeys[0] and vote_pubkeys[1]
        // voted on it
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&duplicate_leaves_descended_from_4[1])
                .unwrap(),
            2 * stake,
        );

        // Add votes, outdating some votes
        assert_eq!(
            heaviest_subtree_fork_choice.add_votes(
                pubkey_votes.iter(),
                bank.epoch_stakes_map(),
                bank.epoch_schedule()
            ),
            higher_child_with_duplicate_parent
        );

        // Duplicate voters should have had all their duplicate voted slot hashes removed
        for duplicate_voter in &[vote_pubkeys[0], vote_pubkeys[2]] {
            let voted_slot_hashes = heaviest_subtree_fork_choice
                .latest_voted_slot_hashes(duplicate_voter)
                .unwrap();
            assert_eq!(voted_slot_hashes, vec![higher_child_with_duplicate_parent]);
        }

        // Duplicate slot should have lost both votes from vote_pubkeys[0] and vote_pubkeys[2],
        // leaving only one vote as compared to before add the votes above
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&duplicate_leaves_descended_from_4[0])
                .unwrap(),
            stake,
        );
        // Weight should be 0, both vote_pubkeys[0] and vote_pubkeys[1]
        // have updated to newer slots
        assert_eq!(
            heaviest_subtree_fork_choice
                .stake_voted_at(&duplicate_leaves_descended_from_4[1])
                .unwrap(),
            0
        );
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            higher_child_with_duplicate_parent
        );
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
        assert!(heaviest_subtree_fork_choice.is_best_child(&(0, Hash::default())));
        assert!(heaviest_subtree_fork_choice.is_best_child(&(4, Hash::default())));

        // 9 is better than 10
        assert!(heaviest_subtree_fork_choice.is_best_child(&(9, Hash::default())));
        assert!(!heaviest_subtree_fork_choice.is_best_child(&(10, Hash::default())));

        // Add new leaf 8, which is better than 9, as both have weight 0
        heaviest_subtree_fork_choice
            .add_new_leaf_slot((8, Hash::default()), Some((4, Hash::default())));
        assert!(heaviest_subtree_fork_choice.is_best_child(&(8, Hash::default())));
        assert!(!heaviest_subtree_fork_choice.is_best_child(&(9, Hash::default())));
        assert!(!heaviest_subtree_fork_choice.is_best_child(&(10, Hash::default())));

        // Add vote for 9, it's the best again
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(3, 100);
        heaviest_subtree_fork_choice.add_votes(
            [(vote_pubkeys[0], (9, Hash::default()))].iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert!(heaviest_subtree_fork_choice.is_best_child(&(9, Hash::default())));
        assert!(!heaviest_subtree_fork_choice.is_best_child(&(8, Hash::default())));
        assert!(!heaviest_subtree_fork_choice.is_best_child(&(10, Hash::default())));
    }

    #[test]
    fn test_merge() {
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(4, stake);
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
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[0], (5, Hash::default())),
            (vote_pubkeys[1], (3, Hash::default())),
            (vote_pubkeys[2], (12, Hash::default())),
        ];
        tree1.add_votes(
            pubkey_votes.iter(),
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
                               slot 20 (vote pubkey 3)
        */
        let forks = tr(10) / (tr(15) / (tr(16) / (tr(17))) / (tr(18) / (tr(19) / (tr(20)))));
        let mut tree2 = HeaviestSubtreeForkChoice::new_from_tree(forks);
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            // more than tree 1
            (vote_pubkeys[0], (16, Hash::default())),
            // more than tree1
            (vote_pubkeys[1], (19, Hash::default())),
            // less than tree1
            (vote_pubkeys[2], (10, Hash::default())),
            // Add a pubkey that only voted on this tree
            (vote_pubkeys[3], (20, Hash::default())),
        ];
        tree2.add_votes(
            pubkey_votes.iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Merge tree2 at leaf 7 of tree1
        tree1.merge(
            tree2,
            &(7, Hash::default()),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Check ancestry information is correct
        let ancestors: Vec<_> = tree1.ancestor_iterator((20, Hash::default())).collect();
        assert_eq!(
            ancestors,
            vec![19, 18, 15, 10, 7, 5, 3, 0]
                .into_iter()
                .map(|s| (s, Hash::default()))
                .collect::<Vec<_>>()
        );
        let ancestors: Vec<_> = tree1.ancestor_iterator((17, Hash::default())).collect();
        assert_eq!(
            ancestors,
            vec![16, 15, 10, 7, 5, 3, 0]
                .into_iter()
                .map(|s| (s, Hash::default()))
                .collect::<Vec<_>>()
        );

        // Check correctness of votes
        // Pubkey 0
        assert_eq!(tree1.stake_voted_at(&(16, Hash::default())).unwrap(), stake);
        assert_eq!(tree1.stake_voted_at(&(5, Hash::default())).unwrap(), 0);
        // Pubkey 1
        assert_eq!(tree1.stake_voted_at(&(19, Hash::default())).unwrap(), stake);
        assert_eq!(tree1.stake_voted_at(&(3, Hash::default())).unwrap(), 0);
        // Pubkey 2
        assert_eq!(tree1.stake_voted_at(&(10, Hash::default())).unwrap(), 0);
        assert_eq!(tree1.stake_voted_at(&(12, Hash::default())).unwrap(), stake);
        // Pubkey 3
        assert_eq!(tree1.stake_voted_at(&(20, Hash::default())).unwrap(), stake);

        for slot in &[0, 3] {
            assert_eq!(
                tree1
                    .stake_voted_subtree(&(*slot, Hash::default()))
                    .unwrap(),
                4 * stake
            );
        }
        for slot in &[5, 7, 10, 15] {
            assert_eq!(
                tree1
                    .stake_voted_subtree(&(*slot, Hash::default()))
                    .unwrap(),
                3 * stake
            );
        }
        for slot in &[18, 19] {
            assert_eq!(
                tree1
                    .stake_voted_subtree(&(*slot, Hash::default()))
                    .unwrap(),
                2 * stake
            );
        }
        for slot in &[9, 11, 12, 16, 20] {
            assert_eq!(
                tree1
                    .stake_voted_subtree(&(*slot, Hash::default()))
                    .unwrap(),
                stake
            );
        }
        for slot in &[17] {
            assert_eq!(
                tree1
                    .stake_voted_subtree(&(*slot, Hash::default()))
                    .unwrap(),
                0
            );
        }

        assert_eq!(tree1.best_overall_slot().0, 20);
    }

    #[test]
    fn test_merge_duplicate() {
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(2, stake);
        /*
            Build fork structure:
                 slot 0
                /     \
           slot 2     slot 5
        */
        let forks = tr(0) / tr(2) / tr(5);
        let duplicate_slot_hash = (5, Hash::default());
        let mut tree1 = HeaviestSubtreeForkChoice::new_from_tree(forks);
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[0], (2, Hash::default())),
            (vote_pubkeys[1], duplicate_slot_hash),
        ];
        tree1.add_votes(
            pubkey_votes.iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        /*
                    Build fork structure:
                          slot 3
                             |
                          slot 5 (heaviest version)
        */
        let duplicate_slot_hash2 = (5, Hash::new_unique());
        let forks = tr((3, Hash::default())) / tr(duplicate_slot_hash2);
        let mut tree2 = HeaviestSubtreeForkChoice::new_from_tree(forks);
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[0], (3, Hash::default())),
            // Pubkey 1 voted on another version of slot 5
            (vote_pubkeys[1], duplicate_slot_hash2),
        ];

        tree2.add_votes(
            pubkey_votes.iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Merge tree2 at leaf 2 of tree1
        tree1.merge(
            tree2,
            &(2, Hash::default()),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Pubkey 1 voted on another version of slot 5 so both should appear in
        // the latest votes
        assert_eq!(tree1.stake_voted_at(&duplicate_slot_hash).unwrap(), stake);
        assert_eq!(tree1.stake_voted_at(&duplicate_slot_hash2).unwrap(), stake);
        assert_eq!(tree1.best_overall_slot(), duplicate_slot_hash2);

        // Check the ancestors are correct
        let ancestors: Vec<_> = tree1.ancestor_iterator(duplicate_slot_hash).collect();
        assert_eq!(ancestors, vec![(0, Hash::default())]);
        let ancestors: Vec<_> = tree1.ancestor_iterator(duplicate_slot_hash2).collect();
        assert_eq!(
            ancestors,
            vec![
                (3, Hash::default()),
                (2, Hash::default()),
                (0, Hash::default())
            ]
        );
    }

    #[test]
    fn test_subtree_diff() {
        let mut heaviest_subtree_fork_choice = setup_forks();

        // Diff of same root is empty, no matter root, intermediate node, or leaf
        assert!(heaviest_subtree_fork_choice
            .subtree_diff((0, Hash::default()), (0, Hash::default()))
            .is_empty());
        assert!(heaviest_subtree_fork_choice
            .subtree_diff((5, Hash::default()), (5, Hash::default()))
            .is_empty());
        assert!(heaviest_subtree_fork_choice
            .subtree_diff((6, Hash::default()), (6, Hash::default()))
            .is_empty());

        // The set reachable from slot 3, excluding subtree 1, is just everything
        // in slot 3 since subtree 1 is an ancestor
        assert_eq!(
            heaviest_subtree_fork_choice.subtree_diff((3, Hash::default()), (1, Hash::default())),
            vec![3, 5, 6]
                .into_iter()
                .map(|s| (s, Hash::default()))
                .collect::<HashSet<_>>()
        );

        // The set reachable from slot 1, excluding subtree 3, is just 1 and
        // the subtree at 2
        assert_eq!(
            heaviest_subtree_fork_choice.subtree_diff((1, Hash::default()), (3, Hash::default())),
            vec![1, 2, 4]
                .into_iter()
                .map(|s| (s, Hash::default()))
                .collect::<HashSet<_>>()
        );

        // The set reachable from slot 1, excluding leaf 6, is just everything
        // except leaf 6
        assert_eq!(
            heaviest_subtree_fork_choice.subtree_diff((0, Hash::default()), (6, Hash::default())),
            vec![0, 1, 3, 5, 2, 4]
                .into_iter()
                .map(|s| (s, Hash::default()))
                .collect::<HashSet<_>>()
        );

        // Set root at 1
        heaviest_subtree_fork_choice.set_root((1, Hash::default()));

        // Zero no longer exists, set reachable from 0 is empty
        assert!(heaviest_subtree_fork_choice
            .subtree_diff((0, Hash::default()), (6, Hash::default()))
            .is_empty());
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
            Some((2, Hash::default()))
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
            Some((2, Hash::default()))
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

    #[test]
    fn test_mark_valid_invalid_forks() {
        let mut heaviest_subtree_fork_choice = setup_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(3, stake);

        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[0], (6, Hash::default())),
            (vote_pubkeys[1], (6, Hash::default())),
            (vote_pubkeys[2], (2, Hash::default())),
        ];
        let expected_best_slot = 6;
        assert_eq!(
            heaviest_subtree_fork_choice.add_votes(
                pubkey_votes.iter(),
                bank.epoch_stakes_map(),
                bank.epoch_schedule()
            ),
            (expected_best_slot, Hash::default()),
        );

        // Mark slot 5 as invalid, the best fork should be its ancestor 3,
        // not the other for at 4.
        let invalid_candidate = (5, Hash::default());
        heaviest_subtree_fork_choice.mark_fork_invalid_candidate(&invalid_candidate);
        assert_eq!(heaviest_subtree_fork_choice.best_overall_slot().0, 3);
        assert!(!heaviest_subtree_fork_choice
            .is_candidate_slot(&invalid_candidate)
            .unwrap());

        // The ancestor is still a candidate
        assert!(heaviest_subtree_fork_choice
            .is_candidate_slot(&(3, Hash::default()))
            .unwrap());

        // Adding another descendant to the invalid candidate won't
        // update the best slot, even if it contains votes
        let new_leaf_slot7 = 7;
        heaviest_subtree_fork_choice.add_new_leaf_slot(
            (new_leaf_slot7, Hash::default()),
            Some((6, Hash::default())),
        );
        let invalid_slot_ancestor = 3;
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot().0,
            invalid_slot_ancestor
        );
        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> =
            vec![(vote_pubkeys[0], (new_leaf_slot7, Hash::default()))];
        assert_eq!(
            heaviest_subtree_fork_choice.add_votes(
                pubkey_votes.iter(),
                bank.epoch_stakes_map(),
                bank.epoch_schedule()
            ),
            (invalid_slot_ancestor, Hash::default()),
        );

        // Adding a descendant to the ancestor of the invalid candidate *should* update
        // the best slot though, since the ancestor is on the heaviest fork
        let new_leaf_slot8 = 8;
        heaviest_subtree_fork_choice.add_new_leaf_slot(
            (new_leaf_slot8, Hash::default()),
            Some((invalid_slot_ancestor, Hash::default())),
        );
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot().0,
            new_leaf_slot8,
        );

        // If we mark slot a descendant of `invalid_candidate` as valid, then that
        // should also mark `invalid_candidate` as valid, and the best slot should
        // be the leaf of the heaviest fork, `new_leaf_slot`.
        heaviest_subtree_fork_choice.mark_fork_valid_candidate(&invalid_candidate);
        assert!(heaviest_subtree_fork_choice
            .is_candidate_slot(&invalid_candidate)
            .unwrap());
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot().0,
            // Should pick the smaller slot of the two new equally weighted leaves
            new_leaf_slot7
        );
    }

    #[test]
    fn test_mark_valid_invalid_forks_duplicate() {
        let (
            mut heaviest_subtree_fork_choice,
            duplicate_leaves_descended_from_4,
            duplicate_leaves_descended_from_5,
        ) = setup_duplicate_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys(3, stake);

        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[0], duplicate_leaves_descended_from_4[0]),
            (vote_pubkeys[1], duplicate_leaves_descended_from_5[0]),
        ];

        heaviest_subtree_fork_choice.add_votes(
            pubkey_votes.iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // The best slot should be the the smallest leaf descended from 4
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            duplicate_leaves_descended_from_4[0]
        );

        // If we mark slot 4 as invalid, the ancestor 2 should be the heaviest, not
        // the other branch at slot 5
        let invalid_candidate = (4, Hash::default());
        heaviest_subtree_fork_choice.mark_fork_invalid_candidate(&invalid_candidate);

        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            (2, Hash::default())
        );

        // Marking candidate as valid again will choose the the heaviest leaf of
        // the newly valid branch
        let duplicate_slot = duplicate_leaves_descended_from_4[0].0;
        let duplicate_descendant = (duplicate_slot + 1, Hash::new_unique());
        heaviest_subtree_fork_choice.add_new_leaf_slot(
            duplicate_descendant,
            Some(duplicate_leaves_descended_from_4[0]),
        );
        heaviest_subtree_fork_choice.mark_fork_valid_candidate(&invalid_candidate);
        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            duplicate_descendant
        );

        // Mark the current heaviest branch as invalid again
        heaviest_subtree_fork_choice.mark_fork_invalid_candidate(&invalid_candidate);

        // If we add a new version of the duplicate slot and votes for that duplicate slot,
        // the new duplicate slot should be picked once it has more weight
        let new_duplicate = (duplicate_slot, Hash::new_unique());
        heaviest_subtree_fork_choice.add_new_leaf_slot(new_duplicate, Some((3, Hash::default())));

        let pubkey_votes: Vec<(Pubkey, SlotHashKey)> = vec![
            (vote_pubkeys[0], new_duplicate),
            (vote_pubkeys[1], new_duplicate),
        ];

        heaviest_subtree_fork_choice.add_votes(
            pubkey_votes.iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        assert_eq!(
            heaviest_subtree_fork_choice.best_overall_slot(),
            new_duplicate
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

    fn setup_duplicate_forks() -> (
        HeaviestSubtreeForkChoice,
        Vec<SlotHashKey>,
        Vec<SlotHashKey>,
    ) {
        /*
                Build fork structure:
                     slot 0
                       |
                     slot 1
                     /       \
                slot 2        |
                   |          slot 3
                slot 4               \
                /    \                slot 5
        slot 10      slot 10        /     |     \
                             slot 6   slot 10   slot 10
            */

        let mut heaviest_subtree_fork_choice = setup_forks();
        let duplicate_slot = 10;
        let mut duplicate_leaves_descended_from_4 =
            std::iter::repeat_with(|| (duplicate_slot, Hash::new_unique()))
                .take(2)
                .collect::<Vec<_>>();
        let mut duplicate_leaves_descended_from_5 =
            std::iter::repeat_with(|| (duplicate_slot, Hash::new_unique()))
                .take(2)
                .collect::<Vec<_>>();
        duplicate_leaves_descended_from_4.sort();
        duplicate_leaves_descended_from_5.sort();

        // Add versions of leaf 10, some with different ancestors, some with the same
        // ancestors
        for duplicate_leaf in &duplicate_leaves_descended_from_4 {
            heaviest_subtree_fork_choice
                .add_new_leaf_slot(*duplicate_leaf, Some((4, Hash::default())));
        }
        for duplicate_leaf in &duplicate_leaves_descended_from_5 {
            heaviest_subtree_fork_choice
                .add_new_leaf_slot(*duplicate_leaf, Some((5, Hash::default())));
        }

        let mut dup_children = heaviest_subtree_fork_choice
            .children(&(4, Hash::default()))
            .unwrap()
            .to_vec();
        dup_children.sort();
        assert_eq!(dup_children, duplicate_leaves_descended_from_4);
        let mut dup_children: Vec<_> = heaviest_subtree_fork_choice
            .children(&(5, Hash::default()))
            .unwrap()
            .iter()
            .copied()
            .filter(|(slot, _)| *slot == duplicate_slot)
            .collect();
        dup_children.sort();
        assert_eq!(dup_children, duplicate_leaves_descended_from_5);

        (
            heaviest_subtree_fork_choice,
            duplicate_leaves_descended_from_4,
            duplicate_leaves_descended_from_5,
        )
    }

    fn check_process_update_correctness<F>(
        heaviest_subtree_fork_choice: &mut HeaviestSubtreeForkChoice,
        pubkey_votes: &[(Pubkey, SlotHashKey)],
        slots_range: Range<Slot>,
        bank: &Bank,
        stake: u64,
        mut expected_best_slot: F,
    ) where
        F: FnMut(Slot, &HeaviestSubtreeForkChoice) -> Slot,
    {
        let unique_votes: HashSet<Slot> = pubkey_votes.iter().map(|(_, (slot, _))| *slot).collect();
        let vote_ancestors: HashMap<Slot, HashSet<SlotHashKey>> = unique_votes
            .iter()
            .map(|v| {
                (
                    *v,
                    heaviest_subtree_fork_choice
                        .ancestor_iterator((*v, Hash::default()))
                        .collect(),
                )
            })
            .collect();
        let mut vote_count: HashMap<Slot, usize> = HashMap::new();
        for (_, vote) in pubkey_votes {
            vote_count
                .entry(vote.0)
                .and_modify(|c| *c += 1)
                .or_insert(1);
        }

        // Maps a slot to the number of descendants of that slot
        // that have been voted on
        let num_voted_descendants: HashMap<Slot, usize> = slots_range
            .clone()
            .map(|slot| {
                let num_voted_descendants = vote_ancestors
                    .iter()
                    .map(|(vote_slot, ancestors)| {
                        (ancestors.contains(&(slot, Hash::default())) || *vote_slot == slot)
                            as usize
                            * vote_count.get(vote_slot).unwrap()
                    })
                    .sum();
                (slot, num_voted_descendants)
            })
            .collect();

        let (update_operations, duplicate_pubkeys_added, duplicate_pubkeys_removed) =
            heaviest_subtree_fork_choice.generate_update_operations(
                pubkey_votes.iter(),
                bank.epoch_stakes_map(),
                bank.epoch_schedule(),
            );

        heaviest_subtree_fork_choice.process_update_operations(
            update_operations,
            &duplicate_pubkeys_added,
            &duplicate_pubkeys_removed,
        );
        for slot in slots_range {
            let expected_stake_voted_at =
                vote_count.get(&slot).cloned().unwrap_or(0) as u64 * stake;
            let expected_stake_voted_subtree =
                *num_voted_descendants.get(&slot).unwrap() as u64 * stake;
            assert_eq!(
                expected_stake_voted_at,
                heaviest_subtree_fork_choice
                    .stake_voted_at(&(slot, Hash::default()))
                    .unwrap()
            );
            assert_eq!(
                expected_stake_voted_subtree,
                heaviest_subtree_fork_choice
                    .stake_voted_subtree(&(slot, Hash::default()))
                    .unwrap()
            );
            assert_eq!(
                expected_best_slot(slot, heaviest_subtree_fork_choice),
                heaviest_subtree_fork_choice
                    .best_slot(&(slot, Hash::default()))
                    .unwrap()
                    .0
            );
        }
    }
}

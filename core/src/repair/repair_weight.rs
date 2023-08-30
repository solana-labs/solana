use {
    crate::{
        consensus::{heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice, tree_diff::TreeDiff},
        repair::{
            repair_generic_traversal::{get_closest_completion, get_unknown_last_index},
            repair_service::{BestRepairsStats, RepairTiming},
            repair_weighted_traversal,
            serve_repair::ShredRepairType,
        },
        replay_stage::DUPLICATE_THRESHOLD,
    },
    solana_ledger::{
        ancestor_iterator::AncestorIterator, blockstore::Blockstore, blockstore_meta::SlotMeta,
    },
    solana_measure::measure::Measure,
    solana_runtime::epoch_stakes::EpochStakes,
    solana_sdk::{
        clock::Slot,
        epoch_schedule::{Epoch, EpochSchedule},
        hash::Hash,
        pubkey::Pubkey,
    },
    std::{
        collections::{HashMap, HashSet, VecDeque},
        iter,
    },
};

#[derive(PartialEq, Eq, Copy, Clone, Hash, Debug)]
enum TreeRoot {
    Root(Slot),
    PrunedRoot(Slot),
}

impl TreeRoot {
    pub fn is_pruned(&self) -> bool {
        matches!(self, Self::PrunedRoot(_))
    }

    #[cfg(test)]
    pub fn slot(&self) -> Slot {
        match self {
            Self::Root(slot) => *slot,
            Self::PrunedRoot(slot) => *slot,
        }
    }
}

impl From<TreeRoot> for Slot {
    fn from(val: TreeRoot) -> Self {
        match val {
            TreeRoot::Root(slot) => slot,
            TreeRoot::PrunedRoot(slot) => slot,
        }
    }
}

#[derive(Clone)]
pub struct RepairWeight {
    // Map from root -> a subtree rooted at that `root`
    trees: HashMap<Slot, HeaviestSubtreeForkChoice>,
    // Map from root -> pruned subtree
    // In the case of duplicate blocks linking back to a slot which is pruned, it is important to
    // hold onto pruned trees so that we can repair / ancestor hashes repair when necessary. Since
    // the parent slot is pruned these blocks will never be replayed / marked dead, so the existing
    // dead duplicate confirmed pathway will not catch this special case.
    // We manage the size by removing slots < root
    pruned_trees: HashMap<Slot, HeaviestSubtreeForkChoice>,

    // Maps each slot to the root of the tree that contains it
    slot_to_tree: HashMap<Slot, TreeRoot>,
    root: Slot,
}

impl RepairWeight {
    pub fn new(root: Slot) -> Self {
        let root_tree = HeaviestSubtreeForkChoice::new((root, Hash::default()));
        let slot_to_tree = HashMap::from([(root, TreeRoot::Root(root))]);
        let trees = HashMap::from([(root, root_tree)]);
        Self {
            trees,
            slot_to_tree,
            root,
            pruned_trees: HashMap::new(),
        }
    }

    pub fn add_votes<I>(
        &mut self,
        blockstore: &Blockstore,
        votes: I,
        epoch_stakes: &HashMap<Epoch, EpochStakes>,
        epoch_schedule: &EpochSchedule,
    ) where
        I: Iterator<Item = (Slot, Vec<Pubkey>)>,
    {
        let mut all_subtree_updates: HashMap<TreeRoot, HashMap<Pubkey, Slot>> = HashMap::new();
        for (slot, pubkey_votes) in votes {
            if slot < self.root {
                continue;
            }
            let mut tree_root = self.get_tree_root(slot);
            let mut new_ancestors = VecDeque::new();
            // If we don't know know  how this slot chains to any existing trees
            // in `self.trees` or `self.pruned_trees`, then use `blockstore` to see if this chains
            // any existing trees in `self.trees`
            if tree_root.is_none() {
                let (discovered_ancestors, existing_subtree_root) =
                    self.find_ancestor_subtree_of_slot(blockstore, slot);
                new_ancestors = discovered_ancestors;
                tree_root = existing_subtree_root;
            }

            let (tree_root, tree) = {
                match (tree_root, *new_ancestors.front().unwrap_or(&slot)) {
                    (Some(tree_root), _) if !tree_root.is_pruned() => (
                        tree_root,
                        self.trees
                            .get_mut(&tree_root.into())
                            .expect("If tree root was found, it must exist in `self.trees`"),
                    ),
                    (Some(tree_root), _) => (
                        tree_root,
                        self.pruned_trees.get_mut(&tree_root.into()).expect(
                            "If a pruned tree root was found, it must exist in `self.pruned_trees`",
                        ),
                    ),
                    (None, earliest_ancestor) => {
                        // There is no known subtree that contains `slot`. Thus, create a new
                        // subtree rooted at the earliest known ancestor of `slot`.
                        // If this earliest known ancestor is not part of the rooted path, create a new
                        // pruned tree from the ancestor that is `> self.root` instead.
                        if earliest_ancestor < self.root {
                            // If the next ancestor exists, it is guaranteed to be `> self.root` because
                            // `find_ancestor_subtree_of_slot` can return at max one ancestor `<
                            // self.root`.
                            let next_earliest_ancestor = *new_ancestors.get(1).unwrap_or(&slot);
                            assert!(next_earliest_ancestor > self.root);
                            // We also guarantee that next_earliest_ancestor does not
                            // already exist as a pruned tree (pre condition for inserting a new
                            // pruned tree) otherwise `tree_root` would not be None.
                            self.insert_new_pruned_tree(next_earliest_ancestor);
                            // Remove `earliest_ancestor` as it should not be added to the tree (we
                            // maintain the invariant that the tree only contains slots >=
                            // `self.root` by checking before `add_new_leaf_slot` and `set_root`)
                            assert_eq!(Some(earliest_ancestor), new_ancestors.pop_front());
                            (
                                TreeRoot::PrunedRoot(next_earliest_ancestor),
                                self.pruned_trees.get_mut(&next_earliest_ancestor).unwrap(),
                            )
                        } else {
                            // We guarantee that `earliest_ancestor` does not already exist in
                            // `self.trees` otherwise `tree_root` would not be None
                            self.insert_new_tree(earliest_ancestor);
                            (
                                TreeRoot::Root(earliest_ancestor),
                                self.trees.get_mut(&earliest_ancestor).unwrap(),
                            )
                        }
                    }
                }
            };

            // First element in `ancestors` must be either:
            // 1) Leaf of some existing subtree
            // 2) Root of new subtree that was just created above through `self.insert_new_tree` or
            //    `self.insert_new_pruned_tree`
            new_ancestors.push_back(slot);
            if new_ancestors.len() > 1 {
                for i in 0..new_ancestors.len() - 1 {
                    // TODO: Repair right now does not distinguish between votes for different
                    // versions of the same slot.
                    tree.add_new_leaf_slot(
                        (new_ancestors[i + 1], Hash::default()),
                        Some((new_ancestors[i], Hash::default())),
                    );
                    self.slot_to_tree.insert(new_ancestors[i + 1], tree_root);
                }
            }

            // Now we know which subtree this slot chains to,
            // add the votes to the list of updates
            let subtree_updates = all_subtree_updates.entry(tree_root).or_default();
            for pubkey in pubkey_votes {
                let cur_max = subtree_updates.entry(pubkey).or_default();
                *cur_max = std::cmp::max(*cur_max, slot);
            }
        }

        for (tree_root, updates) in all_subtree_updates {
            let tree = self
                .get_tree_mut(tree_root)
                .expect("Tree for `tree_root` must exist here");
            let updates: Vec<_> = updates.into_iter().collect();
            tree.add_votes(
                updates
                    .iter()
                    .map(|(pubkey, slot)| (*pubkey, (*slot, Hash::default()))),
                epoch_stakes,
                epoch_schedule,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn get_best_weighted_repairs(
        &mut self,
        blockstore: &Blockstore,
        epoch_stakes: &HashMap<Epoch, EpochStakes>,
        epoch_schedule: &EpochSchedule,
        max_new_orphans: usize,
        max_new_shreds: usize,
        max_unknown_last_index_repairs: usize,
        max_closest_completion_repairs: usize,
        repair_timing: &mut RepairTiming,
        stats: &mut BestRepairsStats,
    ) -> Vec<ShredRepairType> {
        let mut repairs = vec![];
        let mut processed_slots = HashSet::from([self.root]);
        let mut slot_meta_cache = HashMap::default();

        let mut get_best_orphans_elapsed = Measure::start("get_best_orphans");
        // Find the best orphans in order from heaviest stake to least heavy
        self.get_best_orphans(
            blockstore,
            &mut processed_slots,
            &mut repairs,
            epoch_stakes,
            epoch_schedule,
            max_new_orphans,
        );
        // Subtract 1 because the root is not processed as an orphan
        let num_orphan_slots = processed_slots.len() - 1;
        let num_orphan_repairs = repairs.len();
        get_best_orphans_elapsed.stop();

        let mut get_best_shreds_elapsed = Measure::start("get_best_shreds");
        let mut best_shreds_repairs = Vec::default();
        // Find the best incomplete slots in rooted subtree
        self.get_best_shreds(
            blockstore,
            &mut slot_meta_cache,
            &mut best_shreds_repairs,
            max_new_shreds,
        );
        let num_best_shreds_repairs = best_shreds_repairs.len();
        let repair_slots_set: HashSet<Slot> =
            best_shreds_repairs.iter().map(|r| r.slot()).collect();
        let num_best_shreds_slots = repair_slots_set.len();
        processed_slots.extend(repair_slots_set);
        repairs.extend(best_shreds_repairs);
        get_best_shreds_elapsed.stop();

        // Although we have generated repairs for orphan roots and slots in the rooted subtree,
        // if we have space we should generate repairs for slots in orphan trees in preparation for
        // when they are no longer rooted. Here we generate repairs for slots with unknown last
        // indices as well as slots that are close to completion.

        let mut get_unknown_last_index_elapsed = Measure::start("get_unknown_last_index");
        let pre_num_slots = processed_slots.len();
        let unknown_last_index_repairs = self.get_best_unknown_last_index(
            blockstore,
            &mut slot_meta_cache,
            &mut processed_slots,
            max_unknown_last_index_repairs,
        );
        let num_unknown_last_index_repairs = unknown_last_index_repairs.len();
        let num_unknown_last_index_slots = processed_slots.len() - pre_num_slots;
        repairs.extend(unknown_last_index_repairs);
        get_unknown_last_index_elapsed.stop();

        let mut get_closest_completion_elapsed = Measure::start("get_closest_completion");
        let pre_num_slots = processed_slots.len();
        let (closest_completion_repairs, total_slots_processed) = self.get_best_closest_completion(
            blockstore,
            &mut slot_meta_cache,
            &mut processed_slots,
            max_closest_completion_repairs,
        );
        let num_closest_completion_repairs = closest_completion_repairs.len();
        let num_closest_completion_slots = processed_slots.len() - pre_num_slots;
        let num_closest_completion_slots_path =
            total_slots_processed.saturating_sub(num_closest_completion_slots);
        repairs.extend(closest_completion_repairs);
        get_closest_completion_elapsed.stop();

        stats.update(
            num_orphan_slots as u64,
            num_orphan_repairs as u64,
            num_best_shreds_slots as u64,
            num_best_shreds_repairs as u64,
            num_unknown_last_index_slots as u64,
            num_unknown_last_index_repairs as u64,
            num_closest_completion_slots as u64,
            num_closest_completion_slots_path as u64,
            num_closest_completion_repairs as u64,
            self.trees.len() as u64,
        );
        repair_timing.get_best_orphans_elapsed += get_best_orphans_elapsed.as_us();
        repair_timing.get_best_shreds_elapsed += get_best_shreds_elapsed.as_us();
        repair_timing.get_unknown_last_index_elapsed += get_unknown_last_index_elapsed.as_us();
        repair_timing.get_closest_completion_elapsed += get_closest_completion_elapsed.as_us();

        repairs
    }

    /// Split `slot` and descendants into an orphan tree in repair weighting.
    ///
    /// If repair holds a subtree `ST` which contains `slot`, we split `ST` into `(T, T')` where `T'` is
    /// a subtree rooted at `slot` and `T` is what remains (if anything) of `ST` after the split.
    /// If `T` is non-empty, it is inserted back into the set which held the original `ST`
    /// (self.trees for potentially rootable trees, or self.pruned_trees for pruned trees).
    /// `T'` is always inserted into the potentially rootable set `self.trees`.
    /// This function removes and destroys the original `ST`.
    ///
    /// Assumes that `slot` is greater than `self.root`.
    /// Returns slots that were orphaned
    pub fn split_off(&mut self, slot: Slot) -> HashSet<Slot> {
        assert!(slot >= self.root);
        if slot == self.root {
            error!("Trying to orphan root of repair tree {}", slot);
            return HashSet::new();
        }
        match self.slot_to_tree.get(&slot).copied() {
            Some(TreeRoot::Root(subtree_root)) => {
                if subtree_root == slot {
                    info!("{} is already orphan, skipping", slot);
                    return HashSet::new();
                }
                let subtree = self
                    .trees
                    .get_mut(&subtree_root)
                    .expect("`self.slot_to_tree` and `self.trees` must be in sync");
                let orphaned_tree = subtree.split_off(&(slot, Hash::default()));
                self.rename_tree_root(&orphaned_tree, TreeRoot::Root(slot));
                self.trees.insert(slot, orphaned_tree);
                self.trees.get(&slot).unwrap().slots_iter().collect()
            }
            Some(TreeRoot::PrunedRoot(subtree_root)) => {
                // Even if these orphaned slots were previously pruned, they should be added back to
                // `self.trees` as we are no longer sure of their ancestory.
                // After they are repaired there is a chance that they  are now part of the rooted path.
                // This is possible for a duplicate slot with multiple ancestors, if the
                // version we had pruned before had the wrong ancestor, and the correct version is
                // descended from the rooted path.
                // If not they will once again be attached to the pruned set in
                // `update_orphan_ancestors`.

                info!(
                    "Dumping pruned slot {} of tree {} in repair",
                    slot, subtree_root
                );
                let mut subtree = self
                    .pruned_trees
                    .remove(&subtree_root)
                    .expect("`self.slot_to_tree` and `self.pruned_trees` must be in sync");

                if subtree_root == slot {
                    // In this case we simply unprune the entire subtree by adding this subtree
                    // back into the main set of trees, self.trees
                    self.rename_tree_root(&subtree, TreeRoot::Root(subtree_root));
                    self.trees.insert(subtree_root, subtree);
                    self.trees
                        .get(&subtree_root)
                        .unwrap()
                        .slots_iter()
                        .collect()
                } else {
                    let orphaned_tree = subtree.split_off(&(slot, Hash::default()));
                    self.pruned_trees.insert(subtree_root, subtree);
                    self.rename_tree_root(&orphaned_tree, TreeRoot::Root(slot));
                    self.trees.insert(slot, orphaned_tree);
                    self.trees.get(&slot).unwrap().slots_iter().collect()
                }
            }
            None => {
                warn!(
                    "Trying to split off slot {} which doesn't currently exist in repair",
                    slot
                );
                HashSet::new()
            }
        }
    }

    pub fn set_root(&mut self, new_root: Slot) {
        // Roots should be monotonically increasing
        assert!(self.root <= new_root);

        if new_root == self.root {
            return;
        }

        // Root slot of the tree that contains `new_root`, if one exists
        let new_root_tree_root = self
            .slot_to_tree
            .get(&new_root)
            .copied()
            .map(|root| match root {
                TreeRoot::Root(r) => r,
                TreeRoot::PrunedRoot(r) => {
                    panic!("New root {new_root} chains to a pruned tree with root {r}")
                }
            });

        // Purge outdated trees from `self.trees`
        // These are all subtrees that have a `tree_root` < `new_root` and are not part of the
        // rooted subtree
        let subtrees_to_purge: Vec<_> = self
            .trees
            .keys()
            .filter(|subtree_root| {
                **subtree_root < new_root
                    && new_root_tree_root
                        .map(|new_root_tree_root| **subtree_root != new_root_tree_root)
                        .unwrap_or(true)
            })
            .copied()
            .collect();
        for subtree_root in subtrees_to_purge {
            let subtree = self
                .trees
                .remove(&subtree_root)
                .expect("Must exist, was found in `self.trees` above");

            // Track these trees as part of the pruned set
            self.rename_tree_root(&subtree, TreeRoot::PrunedRoot(subtree_root));
            self.pruned_trees.insert(subtree_root, subtree);
        }

        if let Some(new_root_tree_root) = new_root_tree_root {
            let mut new_root_tree = self
                .trees
                .remove(&new_root_tree_root)
                .expect("Found slot root earlier in self.slot_to_trees, tree must exist");

            // Find all descendants of `self.root` that are not reachable from `new_root`.
            // Prune these out and add to `self.pruned_trees`
            trace!("pruning tree {} with {}", new_root_tree_root, new_root);
            let (removed, pruned) = new_root_tree.purge_prune((new_root, Hash::default()));
            for pruned_tree in pruned {
                let pruned_tree_root = pruned_tree.tree_root().0;
                self.rename_tree_root(&pruned_tree, TreeRoot::PrunedRoot(pruned_tree_root));
                self.pruned_trees.insert(pruned_tree_root, pruned_tree);
            }

            for (slot, _) in removed {
                self.slot_to_tree.remove(&slot);
            }

            // Update `self.slot_to_tree` to reflect new root
            new_root_tree.set_tree_root((new_root, Hash::default()));
            self.rename_tree_root(&new_root_tree, TreeRoot::Root(new_root));

            // Insert the tree for the new root
            self.trees.insert(new_root, new_root_tree);
        } else {
            // Guaranteed that `new_root` is not part of `self.trees` because `new_root_tree_root`
            // is None
            self.insert_new_tree(new_root);
        }

        // Clean up the pruned set by trimming slots that are less than `new_root` and removing
        // empty trees
        self.pruned_trees = self
            .pruned_trees
            .drain()
            .flat_map(|(tree_root, mut pruned_tree)| {
                if tree_root < new_root {
                    trace!("pruning tree {} with {}", tree_root, new_root);
                    let (removed, pruned) = pruned_tree.purge_prune((new_root, Hash::default()));
                    for (slot, _) in removed {
                        self.slot_to_tree.remove(&slot);
                    }
                    pruned
                        .into_iter()
                        .chain(iter::once(pruned_tree)) // Add back the original pruned tree
                        .filter(|pruned_tree| !pruned_tree.is_empty()) // Clean up empty trees
                        .map(|new_pruned_subtree| {
                            let new_pruned_tree_root = new_pruned_subtree.tree_root().0;
                            // Resync `self.slot_to_tree`
                            for ((slot, _), _) in new_pruned_subtree.all_slots_stake_voted_subtree()
                            {
                                *self.slot_to_tree.get_mut(slot).unwrap() =
                                    TreeRoot::PrunedRoot(new_pruned_tree_root);
                            }
                            (new_pruned_tree_root, new_pruned_subtree)
                        })
                        .collect()
                } else {
                    vec![(tree_root, pruned_tree)]
                }
            })
            .collect::<HashMap<u64, HeaviestSubtreeForkChoice>>();
        self.root = new_root;
    }

    pub fn root(&self) -> Slot {
        self.root
    }

    // Generate shred repairs for main subtree rooted at `self.root`
    fn get_best_shreds(
        &mut self,
        blockstore: &Blockstore,
        slot_meta_cache: &mut HashMap<Slot, Option<SlotMeta>>,
        repairs: &mut Vec<ShredRepairType>,
        max_new_shreds: usize,
    ) {
        let root_tree = self.trees.get(&self.root).expect("Root tree must exist");
        repair_weighted_traversal::get_best_repair_shreds(
            root_tree,
            blockstore,
            slot_meta_cache,
            repairs,
            max_new_shreds,
        );
    }

    fn get_best_orphans(
        &mut self,
        blockstore: &Blockstore,
        processed_slots: &mut HashSet<Slot>,
        repairs: &mut Vec<ShredRepairType>,
        epoch_stakes: &HashMap<Epoch, EpochStakes>,
        epoch_schedule: &EpochSchedule,
        max_new_orphans: usize,
    ) {
        // Sort each tree in `self.trees`, by the amount of stake that has voted on each,
        // tiebreaker going to earlier slots, thus prioritizing earlier slots on the same fork
        // to ensure replay can continue as soon as possible.
        let mut stake_weighted_trees: Vec<(Slot, u64)> = self
            .trees
            .iter()
            .map(|(slot, tree)| {
                (
                    *slot,
                    tree.stake_voted_subtree(&(*slot, Hash::default()))
                        .expect("Tree must have weight at its own root"),
                )
            })
            .collect();

        // Heavier, smaller slots come first
        Self::sort_by_stake_weight_slot(&mut stake_weighted_trees);
        let mut best_orphans: HashSet<Slot> = HashSet::new();
        for (heaviest_tree_root, _) in stake_weighted_trees {
            if best_orphans.len() >= max_new_orphans {
                break;
            }
            if processed_slots.contains(&heaviest_tree_root) {
                continue;
            }
            // Ignore trees that were merged in a previous iteration
            if self.trees.contains_key(&heaviest_tree_root) {
                let new_orphan_root = self.update_orphan_ancestors(
                    blockstore,
                    heaviest_tree_root,
                    epoch_stakes,
                    epoch_schedule,
                );
                if let Some(new_orphan_root) = new_orphan_root {
                    if new_orphan_root != self.root && !best_orphans.contains(&new_orphan_root) {
                        best_orphans.insert(new_orphan_root);
                        repairs.push(ShredRepairType::Orphan(new_orphan_root));
                        processed_slots.insert(new_orphan_root);
                    }
                }
            }
        }

        // If there are fewer than `max_new_orphans`, just grab the next
        // available ones
        if best_orphans.len() < max_new_orphans {
            for new_orphan in blockstore.orphans_iterator(self.root + 1).unwrap() {
                if !best_orphans.contains(&new_orphan) {
                    repairs.push(ShredRepairType::Orphan(new_orphan));
                    best_orphans.insert(new_orphan);
                    processed_slots.insert(new_orphan);
                }

                if best_orphans.len() == max_new_orphans {
                    break;
                }
            }
        }
    }

    /// For all remaining trees (orphan and rooted), generate repairs for slots missing last_index info
    /// prioritized by # shreds received.
    fn get_best_unknown_last_index(
        &mut self,
        blockstore: &Blockstore,
        slot_meta_cache: &mut HashMap<Slot, Option<SlotMeta>>,
        processed_slots: &mut HashSet<Slot>,
        max_new_repairs: usize,
    ) -> Vec<ShredRepairType> {
        let mut repairs = Vec::default();
        for (_slot, tree) in self.trees.iter() {
            if repairs.len() >= max_new_repairs {
                break;
            }
            let new_repairs = get_unknown_last_index(
                tree,
                blockstore,
                slot_meta_cache,
                processed_slots,
                max_new_repairs - repairs.len(),
            );
            repairs.extend(new_repairs);
        }
        repairs
    }

    /// For all remaining trees (orphan and rooted), generate repairs for subtrees that have last
    /// index info but are missing shreds prioritized by how close to completion they are. These
    /// repairs are also prioritized by age of ancestors, so slots close to completion will first
    /// start by repairing broken ancestors.
    fn get_best_closest_completion(
        &mut self,
        blockstore: &Blockstore,
        slot_meta_cache: &mut HashMap<Slot, Option<SlotMeta>>,
        processed_slots: &mut HashSet<Slot>,
        max_new_repairs: usize,
    ) -> (Vec<ShredRepairType>, /* processed slots */ usize) {
        let mut repairs = Vec::default();
        let mut total_processed_slots = 0;
        for (_slot, tree) in self.trees.iter() {
            if repairs.len() >= max_new_repairs {
                break;
            }
            let (new_repairs, new_processed_slots) = get_closest_completion(
                tree,
                blockstore,
                self.root,
                slot_meta_cache,
                processed_slots,
                max_new_repairs - repairs.len(),
            );
            repairs.extend(new_repairs);
            total_processed_slots += new_processed_slots;
        }
        (repairs, total_processed_slots)
    }

    /// Attempts to chain the orphan subtree rooted at `orphan_tree_root`
    /// to any earlier subtree with new ancestry information in `blockstore`.
    /// Returns the earliest known ancestor of `heaviest_tree_root`.
    fn update_orphan_ancestors(
        &mut self,
        blockstore: &Blockstore,
        mut orphan_tree_root: Slot,
        epoch_stakes: &HashMap<Epoch, EpochStakes>,
        epoch_schedule: &EpochSchedule,
    ) -> Option<Slot> {
        // Must only be called on existing orphan trees
        assert!(self.trees.contains_key(&orphan_tree_root));

        // Check blockstore for any new parents of the heaviest orphan tree. Attempt
        // to chain the orphan back to the main fork rooted at `self.root`.
        while orphan_tree_root != self.root {
            let (new_ancestors, parent_tree_root) =
                self.find_ancestor_subtree_of_slot(blockstore, orphan_tree_root);
            {
                let heaviest_tree = self
                    .trees
                    .get_mut(&orphan_tree_root)
                    .expect("Orphan must exist");

                // Skip the leaf of the parent tree that the orphan would merge
                // with later in a call to `merge_trees`
                let num_skip = usize::from(parent_tree_root.is_some());

                for ancestor in new_ancestors.iter().skip(num_skip).rev() {
                    // We temporarily use orphan_tree_root as the tree root and later
                    // rename tree root to either the parent_tree_root or the earliest_ancestor
                    if *ancestor >= self.root {
                        self.slot_to_tree
                            .insert(*ancestor, TreeRoot::Root(orphan_tree_root));
                        heaviest_tree.add_root_parent((*ancestor, Hash::default()));
                    }
                }
            }
            if let Some(parent_tree_root) = parent_tree_root {
                // If another subtree is discovered to be the parent
                // of this subtree, merge the two.
                self.merge_trees(
                    TreeRoot::Root(orphan_tree_root),
                    parent_tree_root,
                    *new_ancestors
                        .front()
                        .expect("Must exist leaf to merge to if `tree_to_merge`.is_some()"),
                    epoch_stakes,
                    epoch_schedule,
                );
                if let TreeRoot::Root(parent_tree_root) = parent_tree_root {
                    orphan_tree_root = parent_tree_root;
                } else {
                    // This orphan is now part of a pruned tree, no need to chain further
                    return None;
                }
            } else {
                // If there's no other subtree to merge with, then return
                // the current known earliest ancestor of this branch
                if let Some(earliest_ancestor) = new_ancestors.front() {
                    assert!(*earliest_ancestor != self.root); // In this case we would merge above
                    let orphan_tree = self
                        .trees
                        .remove(&orphan_tree_root)
                        .expect("orphan tree must exist");
                    if *earliest_ancestor > self.root {
                        self.rename_tree_root(&orphan_tree, TreeRoot::Root(*earliest_ancestor));
                        assert!(self.trees.insert(*earliest_ancestor, orphan_tree).is_none());
                        orphan_tree_root = *earliest_ancestor;
                    } else {
                        // In this case we should create a new pruned subtree
                        let next_earliest_ancestor =
                            new_ancestors.get(1).unwrap_or(&orphan_tree_root);
                        self.rename_tree_root(
                            &orphan_tree,
                            TreeRoot::PrunedRoot(*next_earliest_ancestor),
                        );
                        assert!(self
                            .pruned_trees
                            .insert(*next_earliest_ancestor, orphan_tree)
                            .is_none());
                        return None;
                    }
                }
                break;
            }
        }

        // Return the (potentially new in the case of some merges)
        // root of this orphan subtree
        Some(orphan_tree_root)
    }

    /// If any pruned trees reach the `DUPLICATE_THRESHOLD`, there is a high chance that they are
    /// duplicate confirmed (can't say for sure because we don't differentiate by hash in
    /// `repair_weight`).
    /// For each such pruned tree, find the deepest child which has reached `DUPLICATE_THRESHOLD`
    /// for handling in ancestor hashes repair
    /// We refer to such trees as "popular" pruned forks, and the deepest child as the "popular" pruned
    /// slot of the fork.
    ///
    /// `DUPLICATE_THRESHOLD` is expected to be > 50%.
    /// It is expected that no two children of a parent could both reach `DUPLICATE_THRESHOLD`.
    pub fn get_popular_pruned_forks(
        &self,
        epoch_stakes: &HashMap<Epoch, EpochStakes>,
        epoch_schedule: &EpochSchedule,
    ) -> Vec<Slot> {
        #[cfg(test)]
        static_assertions::const_assert!(DUPLICATE_THRESHOLD > 0.5);
        let mut repairs = vec![];
        for (pruned_root, pruned_tree) in self.pruned_trees.iter() {
            let mut slot_to_start_repair = (*pruned_root, Hash::default());

            // This pruned tree *could* span an epoch boundary. To be safe we use the
            // minimum DUPLICATE_THRESHOLD across slots in case of stake modification. This
            // *could* lead to a false positive.
            //
            // Additionally, we could have a case where a slot that reached `DUPLICATE_THRESHOLD`
            // no longer reaches threshold post epoch boundary due to stake modifications.
            //
            // Post boundary, we have 2 cases:
            //      1) The previously popular slot stays as the majority fork. In this
            //         case it will eventually reach the new duplicate threshold and
            //         validators missing the correct version will be able to trigger this pruned
            //         repair pathway.
            //      2) With the stake modifications, this previously popular slot no
            //         longer holds the majority stake. The remaining stake is now expected to
            //         reach consensus on a new fork post epoch boundary. Once this consensus is
            //         reached, validators on the popular pruned fork will be able to switch
            //         to the new majority fork.
            //
            // In either case, `HeaviestSubtreeForkChoice` updates the stake only when observing new
            // votes leading to a potential mixed bag of stakes being observed. It is safest to use
            // the minimum threshold from either side of the boundary.
            let min_total_stake = pruned_tree
                .slots_iter()
                .map(|slot| {
                    epoch_stakes
                        .get(&epoch_schedule.get_epoch(slot))
                        .expect("Pruned tree cannot contain slots more than an epoch behind")
                        .total_stake()
                })
                .min()
                .expect("Pruned tree cannot be empty");
            let duplicate_confirmed_threshold =
                ((min_total_stake as f64) * DUPLICATE_THRESHOLD) as u64;

            // TODO: `HeaviestSubtreeForkChoice` subtracts and migrates stake as validators switch
            // forks within the rooted subtree, however `repair_weight` does not migrate stake
            // across subtrees. This could lead to an additional false positive if validators
            // switch post prune as stake added to a pruned tree it is never removed.
            // A further optimization could be to store an additional `latest_votes`
            // in `repair_weight` to manage switching across subtrees.
            if pruned_tree
                .stake_voted_subtree(&slot_to_start_repair)
                .expect("Root of tree must exist")
                >= duplicate_confirmed_threshold
            {
                // Search to find the deepest node that still has >= duplicate_confirmed_threshold (could
                // just use best slot but this is a slight optimization that will save us some iterations
                // in ancestor repair)
                while let Some(child) = pruned_tree
                    .children(&slot_to_start_repair)
                    .expect("Found earlier, this slot should exist")
                    .find(|c| {
                        pruned_tree
                            .stake_voted_subtree(c)
                            .expect("Found in children must exist")
                            >= duplicate_confirmed_threshold
                    })
                {
                    slot_to_start_repair = *child;
                }
                repairs.push(slot_to_start_repair.0);
            }
        }
        repairs
    }

    fn get_tree(&self, tree_root: TreeRoot) -> Option<&HeaviestSubtreeForkChoice> {
        match tree_root {
            TreeRoot::Root(r) => self.trees.get(&r),
            TreeRoot::PrunedRoot(r) => self.pruned_trees.get(&r),
        }
    }

    fn get_tree_mut(&mut self, tree_root: TreeRoot) -> Option<&mut HeaviestSubtreeForkChoice> {
        match tree_root {
            TreeRoot::Root(r) => self.trees.get_mut(&r),
            TreeRoot::PrunedRoot(r) => self.pruned_trees.get_mut(&r),
        }
    }

    fn remove_tree(&mut self, tree_root: TreeRoot) -> Option<HeaviestSubtreeForkChoice> {
        match tree_root {
            TreeRoot::Root(r) => self.trees.remove(&r),
            TreeRoot::PrunedRoot(r) => self.pruned_trees.remove(&r),
        }
    }

    fn get_tree_root(&self, slot: Slot) -> Option<TreeRoot> {
        self.slot_to_tree.get(&slot).copied()
    }

    /// Returns true iff `slot` is currently tracked and in a pruned tree
    pub fn is_pruned(&self, slot: Slot) -> bool {
        self.get_tree_root(slot)
            .as_ref()
            .map(TreeRoot::is_pruned)
            .unwrap_or(false)
    }

    /// Returns true iff `slot1` and `slot2` are both tracked and belong to the same tree
    pub fn same_tree(&self, slot1: Slot, slot2: Slot) -> bool {
        self.get_tree_root(slot1)
            .and_then(|tree_root| self.get_tree(tree_root))
            .map(|tree| tree.contains_block(&(slot2, Hash::default())))
            .unwrap_or(false)
    }

    /// Assumes that `new_tree_root` does not already exist in `self.trees`
    fn insert_new_tree(&mut self, new_tree_root: Slot) {
        assert!(!self.trees.contains_key(&new_tree_root));

        // Update `self.slot_to_tree`
        self.slot_to_tree
            .insert(new_tree_root, TreeRoot::Root(new_tree_root));
        self.trees.insert(
            new_tree_root,
            HeaviestSubtreeForkChoice::new((new_tree_root, Hash::default())),
        );
    }

    /// Assumes that `new_pruned_tree_root` does not already exist in `self.pruned_trees`
    fn insert_new_pruned_tree(&mut self, new_pruned_tree_root: Slot) {
        assert!(!self.pruned_trees.contains_key(&new_pruned_tree_root));

        // Update `self.slot_to_tree`
        self.slot_to_tree.insert(
            new_pruned_tree_root,
            TreeRoot::PrunedRoot(new_pruned_tree_root),
        );
        self.pruned_trees.insert(
            new_pruned_tree_root,
            HeaviestSubtreeForkChoice::new((new_pruned_tree_root, Hash::default())),
        );
    }

    /// Finds any ancestors avaiable from `blockstore` for `slot`.
    /// Ancestor search is stopped when finding one that chains to any
    /// tree in `self.trees` or `self.pruned_trees` or if the ancestor is < self.root.
    ///
    /// Returns ancestors (including the one that triggered the search stop condition),
    /// and possibly the tree_root that `slot` belongs to
    fn find_ancestor_subtree_of_slot(
        &self,
        blockstore: &Blockstore,
        slot: Slot,
    ) -> (VecDeque<Slot>, Option<TreeRoot>) {
        let ancestors = AncestorIterator::new(slot, blockstore);
        let mut ancestors_to_add = VecDeque::new();
        let mut tree = None;
        // This means `heaviest_tree_root` has not been
        // chained back to slots currently being replayed
        // in BankForks. Try to see if blockstore has sufficient
        // information to link this slot back
        for a in ancestors {
            ancestors_to_add.push_front(a);
            // If an ancestor chains back to another subtree or pruned, then return
            tree = self.get_tree_root(a);
            if tree.is_some() || a < self.root {
                break;
            }
        }

        (ancestors_to_add, tree)
    }

    /// Attaches `tree1` rooted at `root1` to `tree2` rooted at `root2`
    /// at the leaf in `tree2` given by `merge_leaf`
    /// Expects `root1` and `root2` to exist in `self.trees` or `self.pruned_trees`
    fn merge_trees(
        &mut self,
        root1: TreeRoot,
        root2: TreeRoot,
        merge_leaf: Slot,
        epoch_stakes: &HashMap<Epoch, EpochStakes>,
        epoch_schedule: &EpochSchedule,
    ) {
        // Update self.slot_to_tree to reflect the merge
        let tree1 = self.remove_tree(root1).expect("tree to merge must exist");
        self.rename_tree_root(&tree1, root2);

        // Merge trees
        let tree2 = self
            .get_tree_mut(root2)
            .expect("tree to be merged into must exist");

        tree2.merge(
            tree1,
            &(merge_leaf, Hash::default()),
            epoch_stakes,
            epoch_schedule,
        );
    }

    // Update all slots in the `tree1` to point to `root2`,
    fn rename_tree_root(&mut self, tree1: &HeaviestSubtreeForkChoice, root2: TreeRoot) {
        let all_slots = tree1.all_slots_stake_voted_subtree();
        for ((slot, _), _) in all_slots {
            *self
                .slot_to_tree
                .get_mut(slot)
                .expect("Nodes in tree must exist in `self.slot_to_tree`") = root2;
        }
    }

    // Heavier, smaller slots come first
    fn sort_by_stake_weight_slot(slot_stake_voted: &mut [(Slot, u64)]) {
        slot_stake_voted.sort_by(|(slot, stake_voted), (slot_, stake_voted_)| {
            if stake_voted == stake_voted_ {
                slot.cmp(slot_)
            } else {
                stake_voted.cmp(stake_voted_).reverse()
            }
        });
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        itertools::Itertools,
        solana_accounts_db::contains::Contains,
        solana_ledger::{
            blockstore::{make_chaining_slot_entries, Blockstore},
            get_tmp_ledger_path,
        },
        solana_runtime::{bank::Bank, bank_utils},
        solana_sdk::hash::Hash,
        trees::tr,
    };

    #[test]
    fn test_sort_by_stake_weight_slot() {
        let mut slots = vec![(3, 30), (2, 30), (5, 31)];
        RepairWeight::sort_by_stake_weight_slot(&mut slots);
        assert_eq!(slots, vec![(5, 31), (2, 30), (3, 30)]);
    }

    #[test]
    fn test_add_votes_invalid() {
        let (blockstore, bank, mut repair_weight) = setup_orphan_repair_weight();
        let root = 3;
        repair_weight.set_root(root);

        // Try to add a vote for slot < root and a slot that is unrooted
        for old_slot in &[2, 4] {
            if *old_slot > root {
                assert!(repair_weight
                    .slot_to_tree
                    .get(old_slot)
                    .unwrap()
                    .is_pruned());
            } else {
                assert!(!repair_weight.slot_to_tree.contains(old_slot));
            }
            let votes = vec![(*old_slot, vec![Pubkey::default()])];
            repair_weight.add_votes(
                &blockstore,
                votes.into_iter(),
                bank.epoch_stakes_map(),
                bank.epoch_schedule(),
            );
            if *old_slot > root {
                assert!(repair_weight.pruned_trees.contains_key(old_slot));
                assert!(repair_weight
                    .slot_to_tree
                    .get(old_slot)
                    .unwrap()
                    .is_pruned());
            } else {
                assert!(!repair_weight.trees.contains_key(old_slot));
                assert!(!repair_weight.slot_to_tree.contains_key(old_slot));
            }
        }
    }

    #[test]
    fn test_add_votes() {
        let blockstore = setup_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(3, stake);
        let votes = vec![(1, vote_pubkeys.clone())];

        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // repair_weight should contain one subtree 0->1
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(
            repair_weight
                .trees
                .get(&0)
                .unwrap()
                .ancestors((1, Hash::default())),
            vec![(0, Hash::default())]
        );
        for i in &[0, 1] {
            assert_eq!(
                *repair_weight.slot_to_tree.get(i).unwrap(),
                TreeRoot::Root(0)
            );
        }

        // Add slots 6 and 4 with the same set of pubkeys,
        // should discover the rest of the tree and the weights,
        // and should only count the latest votes
        let votes = vec![(4, vote_pubkeys.clone()), (6, vote_pubkeys)];
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(
            repair_weight
                .trees
                .get(&0)
                .unwrap()
                .ancestors((4, Hash::default()))
                .into_iter()
                .map(|slot_hash| slot_hash.0)
                .collect::<Vec<_>>(),
            vec![2, 1, 0]
        );
        assert_eq!(
            repair_weight
                .trees
                .get(&0)
                .unwrap()
                .ancestors((6, Hash::default()))
                .into_iter()
                .map(|slot_hash| slot_hash.0)
                .collect::<Vec<_>>(),
            vec![5, 3, 1, 0]
        );
        for slot in 0..=6 {
            assert_eq!(
                *repair_weight.slot_to_tree.get(&slot).unwrap(),
                TreeRoot::Root(0)
            );
            let stake_voted_at = repair_weight
                .trees
                .get(&0)
                .unwrap()
                .stake_voted_at(&(slot, Hash::default()))
                .unwrap();
            if slot == 6 {
                assert_eq!(stake_voted_at, 3 * stake);
            } else {
                assert_eq!(stake_voted_at, 0);
            }
        }

        for slot in &[6, 5, 3, 1, 0] {
            let stake_voted_subtree = repair_weight
                .trees
                .get(&0)
                .unwrap()
                .stake_voted_subtree(&(*slot, Hash::default()))
                .unwrap();
            assert_eq!(stake_voted_subtree, 3 * stake);
        }
        for slot in &[4, 2] {
            let stake_voted_subtree = repair_weight
                .trees
                .get(&0)
                .unwrap()
                .stake_voted_subtree(&(*slot, Hash::default()))
                .unwrap();
            assert_eq!(stake_voted_subtree, 0);
        }
    }

    #[test]
    fn test_add_votes_orphans() {
        let blockstore = setup_orphans();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(3, stake);
        let votes = vec![(1, vote_pubkeys.clone()), (8, vote_pubkeys.clone())];

        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Should contain two trees, one for main fork, one for the orphan
        // branch
        assert_eq!(repair_weight.trees.len(), 2);
        assert_eq!(
            repair_weight
                .trees
                .get(&0)
                .unwrap()
                .ancestors((1, Hash::default())),
            vec![(0, Hash::default())]
        );
        assert!(repair_weight
            .trees
            .get(&8)
            .unwrap()
            .ancestors((8, Hash::default()))
            .is_empty());

        let votes = vec![(1, vote_pubkeys.clone()), (10, vote_pubkeys.clone())];
        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Should contain two trees, one for main fork, one for the orphan
        // branch
        assert_eq!(repair_weight.trees.len(), 2);
        assert_eq!(
            repair_weight
                .trees
                .get(&0)
                .unwrap()
                .ancestors((1, Hash::default())),
            vec![(0, Hash::default())]
        );
        assert_eq!(
            repair_weight
                .trees
                .get(&8)
                .unwrap()
                .ancestors((10, Hash::default())),
            vec![(8, Hash::default())]
        );

        // Connect orphan back to main fork
        blockstore.add_tree(tr(6) / (tr(8)), true, true, 2, Hash::default());
        assert_eq!(
            AncestorIterator::new(8, &blockstore).collect::<Vec<_>>(),
            vec![6, 5, 3, 1, 0]
        );
        let votes = vec![(11, vote_pubkeys)];

        // Should not resolve orphans because `update_orphan_ancestors` has
        // not been called, but should add to the orphan branch
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(
            repair_weight
                .trees
                .get(&8)
                .unwrap()
                .ancestors((11, Hash::default()))
                .into_iter()
                .map(|slot_hash| slot_hash.0)
                .collect::<Vec<_>>(),
            vec![10, 8]
        );

        for slot in &[8, 10, 11] {
            assert_eq!(
                *repair_weight.slot_to_tree.get(slot).unwrap(),
                TreeRoot::Root(8)
            );
        }
        for slot in 0..=1 {
            assert_eq!(
                *repair_weight.slot_to_tree.get(&slot).unwrap(),
                TreeRoot::Root(0)
            );
        }

        // Call `update_orphan_ancestors` to resolve orphan
        repair_weight.update_orphan_ancestors(
            &blockstore,
            8,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        for slot in &[8, 10, 11] {
            assert_eq!(
                *repair_weight.slot_to_tree.get(slot).unwrap(),
                TreeRoot::Root(0)
            );
        }
        assert_eq!(repair_weight.trees.len(), 1);
        assert!(repair_weight.trees.contains_key(&0));
    }

    #[test]
    fn test_add_votes_pruned() {
        // Connect orphan to main fork
        let blockstore = setup_orphans();
        blockstore.add_tree(tr(4) / tr(8), true, true, 2, Hash::default());

        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(3, stake);
        let votes = vec![(6, vote_pubkeys.clone()), (11, vote_pubkeys.clone())];

        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Should contain rooted tree
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.pruned_trees.len(), 0);
        assert_eq!(
            *repair_weight.trees.get(&0).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(
                tr(0)
                    / (tr(1)
                        / (tr(2) / (tr(4) / (tr(8) / (tr(10) / tr(11)))))
                        / (tr(3) / (tr(5) / tr(6))))
            )
        );

        // Update root to create a pruned tree
        repair_weight.set_root(2);
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.pruned_trees.len(), 1);

        // Add a vote to a slot chaining to pruned
        blockstore.add_tree(tr(6) / tr(20), true, true, 2, Hash::default());
        let votes = vec![(23, vote_pubkeys.iter().take(1).copied().collect_vec())];
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Should be part of the now pruned tree
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.pruned_trees.len(), 1);
        assert_eq!(
            *repair_weight.trees.get(&2).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(2) / (tr(4) / (tr(8) / (tr(10) / tr(11)))))
        );
        assert_eq!(
            *repair_weight.pruned_trees.get(&3).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(
                tr(3) / (tr(5) / (tr(6) / (tr(20) / (tr(22) / tr(23)))))
            )
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&23).unwrap(),
            TreeRoot::PrunedRoot(3)
        );

        // Pruned tree should now have 1 vote
        assert_eq!(
            repair_weight
                .pruned_trees
                .get(&3)
                .unwrap()
                .stake_voted_subtree(&(3, Hash::default()))
                .unwrap(),
            stake
        );
        assert_eq!(
            repair_weight
                .trees
                .get(&2)
                .unwrap()
                .stake_voted_subtree(&(2, Hash::default()))
                .unwrap(),
            3 * stake
        );

        // Add the rest of the stake
        let votes = vec![(23, vote_pubkeys.iter().skip(1).copied().collect_vec())];
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Pruned tree should have all the stake as well
        assert_eq!(
            repair_weight
                .pruned_trees
                .get(&3)
                .unwrap()
                .stake_voted_subtree(&(3, Hash::default()))
                .unwrap(),
            3 * stake
        );
        assert_eq!(
            repair_weight
                .trees
                .get(&2)
                .unwrap()
                .stake_voted_subtree(&(2, Hash::default()))
                .unwrap(),
            3 * stake
        );

        // Update root and trim pruned tree
        repair_weight.set_root(10);
        // Add a vote to an orphan, where earliest ancestor is unrooted, should still add as pruned
        blockstore.add_tree(
            tr(7) / (tr(9) / (tr(12) / tr(13))),
            true,
            true,
            2,
            Hash::default(),
        );
        let votes = vec![(13, vote_pubkeys)];
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.pruned_trees.len(), 2);
        assert_eq!(
            *repair_weight.trees.get(&10).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(10) / tr(11))
        );
        assert_eq!(
            *repair_weight.pruned_trees.get(&12).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(12) / tr(13))
        );
        assert_eq!(
            *repair_weight.pruned_trees.get(&20).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(20) / (tr(22) / tr(23)))
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&13).unwrap(),
            TreeRoot::PrunedRoot(12)
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&23).unwrap(),
            TreeRoot::PrunedRoot(20)
        );
    }

    #[test]
    fn test_update_orphan_ancestors() {
        let blockstore = setup_orphans();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(3, stake);
        // Create votes for both orphan branches
        let votes = vec![
            (10, vote_pubkeys[0..1].to_vec()),
            (22, vote_pubkeys[1..3].to_vec()),
        ];

        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        assert_eq!(repair_weight.trees.len(), 3);
        // Roots of the orphan branches should exist
        assert!(repair_weight.trees.contains_key(&0));
        assert!(repair_weight.trees.contains_key(&8));
        assert!(repair_weight.trees.contains_key(&20));

        // Call `update_orphan_ancestors` to resolve orphan
        repair_weight.update_orphan_ancestors(
            &blockstore,
            8,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Nothing has changed because no orphans were
        // resolved
        assert_eq!(repair_weight.trees.len(), 3);
        // Roots of the orphan branches should exist
        assert!(repair_weight.trees.contains_key(&0));
        assert!(repair_weight.trees.contains_key(&8));
        assert!(repair_weight.trees.contains_key(&20));

        // Resolve orphans in blockstore
        blockstore.add_tree(tr(6) / (tr(8)), true, true, 2, Hash::default());
        blockstore.add_tree(tr(11) / (tr(20)), true, true, 2, Hash::default());
        // Call `update_orphan_ancestors` to resolve orphan
        repair_weight.update_orphan_ancestors(
            &blockstore,
            20,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Only the main fork should exist now
        assert_eq!(repair_weight.trees.len(), 1);
        assert!(repair_weight.trees.contains_key(&0));
    }

    #[test]
    fn test_get_best_orphans() {
        let blockstore = setup_orphans();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(2, stake);
        let votes = vec![(8, vec![vote_pubkeys[0]]), (20, vec![vote_pubkeys[1]])];
        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Ask for only 1 orphan. Because the orphans have the same weight,
        // should prioritize smaller orphan first
        let mut repairs = vec![];
        let mut processed_slots: HashSet<Slot> = vec![repair_weight.root].into_iter().collect();
        repair_weight.get_best_orphans(
            &blockstore,
            &mut processed_slots,
            &mut repairs,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
            1,
        );
        assert_eq!(
            repair_weight
                .trees
                .get(&8)
                .unwrap()
                .stake_voted_subtree(&(8, Hash::default()))
                .unwrap(),
            repair_weight
                .trees
                .get(&20)
                .unwrap()
                .stake_voted_subtree(&(20, Hash::default()))
                .unwrap()
        );
        assert_eq!(repairs.len(), 1);
        assert_eq!(repairs[0].slot(), 8);

        // New vote on same orphan branch, without any new slot chaining
        // information blockstore should not resolve the orphan
        repairs = vec![];
        processed_slots = vec![repair_weight.root].into_iter().collect();
        let votes = vec![(10, vec![vote_pubkeys[0]])];
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        repair_weight.get_best_orphans(
            &blockstore,
            &mut processed_slots,
            &mut repairs,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
            1,
        );
        assert_eq!(repairs.len(), 1);
        assert_eq!(repairs[0].slot(), 8);

        // Ask for 2 orphans, should return all the orphans
        repairs = vec![];
        processed_slots = vec![repair_weight.root].into_iter().collect();
        repair_weight.get_best_orphans(
            &blockstore,
            &mut processed_slots,
            &mut repairs,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
            2,
        );
        assert_eq!(repairs.len(), 2);
        assert_eq!(repairs[0].slot(), 8);
        assert_eq!(repairs[1].slot(), 20);

        // If one orphan gets heavier, should pick that one
        repairs = vec![];
        processed_slots = vec![repair_weight.root].into_iter().collect();
        let votes = vec![(20, vec![vote_pubkeys[0]])];
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        repair_weight.get_best_orphans(
            &blockstore,
            &mut processed_slots,
            &mut repairs,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
            1,
        );
        assert_eq!(repairs.len(), 1);
        assert_eq!(repairs[0].slot(), 20);

        // Resolve the orphans, should now return no
        // orphans
        repairs = vec![];
        processed_slots = vec![repair_weight.root].into_iter().collect();
        blockstore.add_tree(tr(6) / (tr(8)), true, true, 2, Hash::default());
        blockstore.add_tree(tr(11) / (tr(20)), true, true, 2, Hash::default());
        repair_weight.get_best_orphans(
            &blockstore,
            &mut processed_slots,
            &mut repairs,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
            1,
        );
        assert!(repairs.is_empty());
    }

    #[test]
    fn test_get_extra_orphans() {
        let blockstore = setup_orphans();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(2, stake);
        let votes = vec![(8, vec![vote_pubkeys[0]])];
        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Should only be one orphan in the `trees` map
        assert_eq!(repair_weight.trees.len(), 2);
        // Roots of the orphan branches should exist
        assert!(repair_weight.trees.contains_key(&0));
        assert!(repair_weight.trees.contains_key(&8));

        // Ask for 2 orphans. Even though there's only one
        // orphan in the `trees` map, we should search for
        // exactly one more of the remaining two
        let mut repairs = vec![];
        let mut processed_slots: HashSet<Slot> = vec![repair_weight.root].into_iter().collect();
        blockstore.add_tree(tr(100) / (tr(101)), true, true, 2, Hash::default());
        repair_weight.get_best_orphans(
            &blockstore,
            &mut processed_slots,
            &mut repairs,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
            2,
        );
        assert_eq!(repairs.len(), 2);
        assert_eq!(repairs[0].slot(), 8);
        assert_eq!(repairs[1].slot(), 20);

        // If we ask for 3 orphans, we should get all of them
        let mut repairs = vec![];
        processed_slots = vec![repair_weight.root].into_iter().collect();
        repair_weight.get_best_orphans(
            &blockstore,
            &mut processed_slots,
            &mut repairs,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
            3,
        );
        assert_eq!(repairs.len(), 3);
        assert_eq!(repairs[0].slot(), 8);
        assert_eq!(repairs[1].slot(), 20);
        assert_eq!(repairs[2].slot(), 100);
    }

    #[test]
    fn test_set_root() {
        let (_, _, mut repair_weight) = setup_orphan_repair_weight();

        // Set root at 1
        repair_weight.set_root(1);
        check_old_root_purged_verify_new_root(0, 1, &repair_weight);
        assert!(repair_weight.pruned_trees.is_empty());

        // Other later slots in the fork should be updated to map to the
        // the new root
        assert_eq!(
            *repair_weight.slot_to_tree.get(&1).unwrap(),
            TreeRoot::Root(1)
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&2).unwrap(),
            TreeRoot::Root(1)
        );

        // Trees tracked should be updated
        assert_eq!(repair_weight.trees.get(&1).unwrap().tree_root().0, 1);

        // Orphan slots should not be changed
        for orphan in &[8, 20] {
            assert_eq!(
                repair_weight.trees.get(orphan).unwrap().tree_root().0,
                *orphan
            );
            assert_eq!(
                *repair_weight.slot_to_tree.get(orphan).unwrap(),
                TreeRoot::Root(*orphan)
            );
        }
    }

    #[test]
    fn test_set_missing_root() {
        let (_, _, mut repair_weight) = setup_orphan_repair_weight();

        // Make sure the slot to root has not been added to the set
        let missing_root_slot = 5;
        assert!(!repair_weight.slot_to_tree.contains_key(&missing_root_slot));

        // Set root at 5
        repair_weight.set_root(missing_root_slot);
        check_old_root_purged_verify_new_root(0, missing_root_slot, &repair_weight);

        // Should purge [0, 5) from tree
        for slot in 0..5 {
            assert!(!repair_weight.slot_to_tree.contains_key(&slot));
        }

        // Orphan slots should not be changed
        for orphan in &[8, 20] {
            assert_eq!(
                repair_weight.trees.get(orphan).unwrap().tree_root().0,
                *orphan
            );
            assert_eq!(
                *repair_weight.slot_to_tree.get(orphan).unwrap(),
                TreeRoot::Root(*orphan)
            );
        }
    }

    #[test]
    fn test_set_root_existing_non_root_tree() {
        let (_, _, mut repair_weight) = setup_orphan_repair_weight();

        // Set root in an existing orphan branch, slot 10
        repair_weight.set_root(10);
        check_old_root_purged_verify_new_root(0, 10, &repair_weight);

        // Should purge old root tree [0, 6]
        for slot in 0..6 {
            assert!(!repair_weight.slot_to_tree.contains_key(&slot));
        }

        // Should purge orphan parent as well
        assert!(!repair_weight.slot_to_tree.contains_key(&8));

        // Other higher orphan branch rooted at slot `20` remains unchanged
        assert_eq!(repair_weight.trees.get(&20).unwrap().tree_root().0, 20);
        assert_eq!(
            *repair_weight.slot_to_tree.get(&20).unwrap(),
            TreeRoot::Root(20)
        );
    }

    #[test]
    fn test_set_root_check_pruned_slots() {
        let (blockstore, bank, mut repair_weight) = setup_orphan_repair_weight();

        // Chain orphan 8 back to the main fork, but don't
        // touch orphan 20
        blockstore.add_tree(tr(4) / (tr(8)), true, true, 2, Hash::default());

        // Call `update_orphan_ancestors` to resolve orphan
        repair_weight.update_orphan_ancestors(
            &blockstore,
            8,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Set a root at 3
        repair_weight.set_root(3);
        check_old_root_purged_verify_new_root(0, 3, &repair_weight);

        // Setting root at 3 should purge all slots < 3 and prune slots > 3 that are not part of
        // the rooted path. Additionally slot 4 should now be a standalone pruned tree
        for purged_slot in 0..3 {
            assert!(!repair_weight.slot_to_tree.contains_key(&purged_slot));
            assert!(!repair_weight.trees.contains_key(&purged_slot));
        }
        for pruned_slot in &[4, 8, 10] {
            assert!(repair_weight
                .slot_to_tree
                .get(pruned_slot)
                .unwrap()
                .is_pruned());
        }
        assert_eq!(
            repair_weight.pruned_trees.keys().copied().collect_vec(),
            vec![4]
        );

        // Orphan 20 should still exist
        assert_eq!(repair_weight.trees.len(), 2);
        assert_eq!(repair_weight.trees.get(&20).unwrap().tree_root().0, 20);
        assert_eq!(
            *repair_weight.slot_to_tree.get(&20).unwrap(),
            TreeRoot::Root(20)
        );

        // Now set root at a slot 30 that doesnt exist in `repair_weight`, but is
        // higher than the remaining orphan
        assert!(!repair_weight.slot_to_tree.contains_key(&30));
        repair_weight.set_root(30);
        check_old_root_purged_verify_new_root(3, 30, &repair_weight);

        // Previously pruned tree should now also be purged
        assert_eq!(repair_weight.pruned_trees.len(), 0);

        // Trees tracked should be updated
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.slot_to_tree.len(), 1);
        assert_eq!(repair_weight.root, 30);
        assert_eq!(repair_weight.trees.get(&30).unwrap().tree_root().0, 30);
    }

    #[test]
    fn test_set_root_pruned_tree_trim_and_cleanup() {
        let blockstore = setup_big_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(3, stake);
        let votes = vec![
            (4, vote_pubkeys.clone()),
            (6, vote_pubkeys.clone()),
            (11, vote_pubkeys.clone()),
            (23, vote_pubkeys),
        ];

        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Should be 1 rooted tree
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.pruned_trees.len(), 0);

        // Set root to 3, should now prune 4 and 8 trees
        repair_weight.set_root(3);
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.pruned_trees.len(), 2);
        assert_eq!(
            *repair_weight.trees.get(&3).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(
                tr(3) / (tr(5) / tr(6)) / (tr(9) / (tr(20) / (tr(22) / tr(23))))
            )
        );
        assert_eq!(
            *repair_weight.pruned_trees.get(&4).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(4))
        );
        assert_eq!(
            *repair_weight.pruned_trees.get(&8).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(8) / (tr(10) / tr(11)))
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&6).unwrap(),
            TreeRoot::Root(3)
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&23).unwrap(),
            TreeRoot::Root(3)
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&4).unwrap(),
            TreeRoot::PrunedRoot(4)
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&11).unwrap(),
            TreeRoot::PrunedRoot(8)
        );

        // Set root to 9,
        //  5, 6 should be purged
        //  Pruned tree 4 should be cleaned up entirely
        //  Pruned tree 8 should be trimmed to 10
        repair_weight.set_root(9);
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.pruned_trees.len(), 1);
        assert_eq!(
            *repair_weight.trees.get(&9).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(9) / (tr(20) / (tr(22) / tr(23))))
        );
        assert_eq!(
            *repair_weight.pruned_trees.get(&10).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(10) / tr(11))
        );
        assert!(!repair_weight.slot_to_tree.contains(&6));
        assert_eq!(
            *repair_weight.slot_to_tree.get(&23).unwrap(),
            TreeRoot::Root(9)
        );
        assert!(!repair_weight.slot_to_tree.contains(&4));
        assert_eq!(
            *repair_weight.slot_to_tree.get(&11).unwrap(),
            TreeRoot::PrunedRoot(10)
        );

        // Set root to 20,
        //  Pruned tree 10 (formerly 8) should be cleaned up entirely
        repair_weight.set_root(20);
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.pruned_trees.len(), 0);
        assert_eq!(
            *repair_weight.trees.get(&20).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(20) / (tr(22) / tr(23)))
        );
        assert!(!repair_weight.slot_to_tree.contains(&6));
        assert_eq!(
            *repair_weight.slot_to_tree.get(&23).unwrap(),
            TreeRoot::Root(20)
        );
        assert!(!repair_weight.slot_to_tree.contains(&4));
        assert!(!repair_weight.slot_to_tree.contains(&11));
    }

    #[test]
    fn test_set_root_pruned_tree_split() {
        let blockstore = setup_big_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(3, stake);
        let votes = vec![
            (4, vote_pubkeys.clone()),
            (6, vote_pubkeys.clone()),
            (11, vote_pubkeys.clone()),
            (23, vote_pubkeys),
        ];

        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Should be 1 rooted tree
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.pruned_trees.len(), 0);

        // Set root to 2, subtree at 3 should be pruned
        repair_weight.set_root(2);
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.pruned_trees.len(), 1);

        assert_eq!(
            *repair_weight.trees.get(&2).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(2) / tr(4) / (tr(8) / (tr(10) / tr(11))))
        );
        assert_eq!(
            *repair_weight.pruned_trees.get(&3).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(
                tr(3) / (tr(5) / tr(6)) / (tr(9) / (tr(20) / (tr(22) / tr(23))))
            )
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&4).unwrap(),
            TreeRoot::Root(2)
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&11).unwrap(),
            TreeRoot::Root(2)
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&6).unwrap(),
            TreeRoot::PrunedRoot(3)
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&23).unwrap(),
            TreeRoot::PrunedRoot(3)
        );

        // Now update root to 4
        //  Subtree at 8 will now be pruned
        //  Pruned subtree at 3 will now be *split* into 2 pruned subtrees 5, and 9
        repair_weight.set_root(4);
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.pruned_trees.len(), 3);

        assert_eq!(
            *repair_weight.trees.get(&4).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(4))
        );
        assert_eq!(
            *repair_weight.pruned_trees.get(&8).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(8) / (tr(10) / tr(11)))
        );
        assert_eq!(
            *repair_weight.pruned_trees.get(&5).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(5) / tr(6))
        );
        assert_eq!(
            *repair_weight.pruned_trees.get(&9).unwrap(),
            HeaviestSubtreeForkChoice::new_from_tree(tr(9) / (tr(20) / (tr(22) / tr(23))))
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&4).unwrap(),
            TreeRoot::Root(4)
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&11).unwrap(),
            TreeRoot::PrunedRoot(8)
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&6).unwrap(),
            TreeRoot::PrunedRoot(5)
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&23).unwrap(),
            TreeRoot::PrunedRoot(9)
        );
    }

    #[test]
    fn test_add_votes_update_orphans_unrooted() {
        let root = 3;
        // Test chaining back to slots that were purged when the root 3 was set.
        // 1) Slot 2 < root should be purged and subsequent children purged
        // 2) Slot 4 > root should be pruned and subsequent children pruned
        for old_parent in &[2, 4] {
            let (blockstore, bank, mut repair_weight) = setup_orphan_repair_weight();
            // Set a root at 3
            repair_weight.set_root(root);

            // Check
            assert_eq!(repair_weight.pruned_trees.len(), 1);
            if *old_parent > root {
                assert!(repair_weight
                    .slot_to_tree
                    .get(old_parent)
                    .unwrap()
                    .is_pruned());
                assert_eq!(
                    repair_weight
                        .pruned_trees
                        .get(old_parent)
                        .unwrap()
                        .tree_root()
                        .0,
                    *old_parent
                );
                assert_eq!(
                    *repair_weight.slot_to_tree.get(old_parent).unwrap(),
                    TreeRoot::PrunedRoot(*old_parent)
                );
            } else {
                assert!(!repair_weight.slot_to_tree.contains_key(old_parent));
                assert!(!repair_weight.trees.contains_key(old_parent));
                assert!(!repair_weight.pruned_trees.contains_key(old_parent));
            }

            // Chain orphan 8 back to slot `old_parent`
            blockstore.add_tree(tr(*old_parent) / (tr(8)), true, true, 2, Hash::default());

            // Chain orphan 20 back to orphan 8
            blockstore.add_tree(tr(8) / (tr(20)), true, true, 2, Hash::default());

            // Call `update_orphan_ancestors` to resolve orphan
            repair_weight.update_orphan_ancestors(
                &blockstore,
                20,
                bank.epoch_stakes_map(),
                bank.epoch_schedule(),
            );

            // Should prune entire branch
            for slot in &[*old_parent, 8, 10, 20] {
                if *slot > root {
                    assert!(repair_weight.slot_to_tree.get(slot).unwrap().is_pruned());
                } else {
                    assert!(!repair_weight.slot_to_tree.contains_key(slot));
                }
            }
            if *old_parent > root {
                assert_eq!(repair_weight.pruned_trees.len(), 1);

                assert_eq!(
                    repair_weight
                        .pruned_trees
                        .get(old_parent)
                        .unwrap()
                        .tree_root()
                        .0,
                    *old_parent
                );
                assert_eq!(
                    *repair_weight.slot_to_tree.get(old_parent).unwrap(),
                    TreeRoot::PrunedRoot(*old_parent)
                );
                assert_eq!(
                    *repair_weight.slot_to_tree.get(&8).unwrap(),
                    TreeRoot::PrunedRoot(*old_parent)
                );
                assert_eq!(
                    *repair_weight.slot_to_tree.get(&10).unwrap(),
                    TreeRoot::PrunedRoot(*old_parent)
                );
                assert_eq!(
                    *repair_weight.slot_to_tree.get(&20).unwrap(),
                    TreeRoot::PrunedRoot(*old_parent)
                );
            } else {
                // Should create a new pruned tree alongside 4
                assert_eq!(repair_weight.pruned_trees.len(), 2);

                assert_eq!(repair_weight.pruned_trees.get(&4).unwrap().tree_root().0, 4);
                assert_eq!(
                    *repair_weight.slot_to_tree.get(&4).unwrap(),
                    TreeRoot::PrunedRoot(4)
                );

                assert_eq!(repair_weight.pruned_trees.get(&8).unwrap().tree_root().0, 8);
                assert_eq!(
                    *repair_weight.slot_to_tree.get(&8).unwrap(),
                    TreeRoot::PrunedRoot(8)
                );
                assert_eq!(
                    *repair_weight.slot_to_tree.get(&10).unwrap(),
                    TreeRoot::PrunedRoot(8)
                );
                assert_eq!(
                    *repair_weight.slot_to_tree.get(&20).unwrap(),
                    TreeRoot::PrunedRoot(8)
                );

                // Old parent remains missing
                assert!(!repair_weight.slot_to_tree.contains_key(old_parent));
                assert!(!repair_weight.trees.contains_key(old_parent));
                assert!(!repair_weight.pruned_trees.contains_key(old_parent));
            }

            // Add a vote that chains back to `old_parent`, should be added to appropriate pruned
            // tree
            let new_vote_slot = 100;
            blockstore.add_tree(
                tr(*old_parent) / tr(new_vote_slot),
                true,
                true,
                2,
                Hash::default(),
            );
            repair_weight.add_votes(
                &blockstore,
                vec![(new_vote_slot, vec![Pubkey::default()])].into_iter(),
                bank.epoch_stakes_map(),
                bank.epoch_schedule(),
            );

            assert!(repair_weight
                .slot_to_tree
                .get(&new_vote_slot)
                .unwrap()
                .is_pruned());
            if *old_parent > root {
                // Adds to new tree
                assert_eq!(repair_weight.pruned_trees.len(), 1);
                assert_eq!(
                    repair_weight
                        .pruned_trees
                        .get(old_parent)
                        .unwrap()
                        .tree_root()
                        .0,
                    *old_parent
                );
                assert_eq!(
                    *repair_weight.slot_to_tree.get(&new_vote_slot).unwrap(),
                    TreeRoot::PrunedRoot(*old_parent)
                );
            } else {
                // Creates new tree
                assert_eq!(repair_weight.pruned_trees.len(), 3);

                assert_eq!(repair_weight.pruned_trees.get(&4).unwrap().tree_root().0, 4);
                assert_eq!(
                    *repair_weight.slot_to_tree.get(&4).unwrap(),
                    TreeRoot::PrunedRoot(4)
                );

                assert_eq!(repair_weight.pruned_trees.get(&8).unwrap().tree_root().0, 8);
                assert_eq!(
                    *repair_weight.slot_to_tree.get(&8).unwrap(),
                    TreeRoot::PrunedRoot(8)
                );
                assert_eq!(
                    *repair_weight.slot_to_tree.get(&10).unwrap(),
                    TreeRoot::PrunedRoot(8)
                );
                assert_eq!(
                    *repair_weight.slot_to_tree.get(&20).unwrap(),
                    TreeRoot::PrunedRoot(8)
                );

                assert_eq!(
                    repair_weight
                        .pruned_trees
                        .get(&new_vote_slot)
                        .unwrap()
                        .tree_root()
                        .0,
                    new_vote_slot
                );
                assert_eq!(
                    *repair_weight.slot_to_tree.get(&new_vote_slot).unwrap(),
                    TreeRoot::PrunedRoot(new_vote_slot)
                );

                // Old parent remains missing
                assert!(!repair_weight.slot_to_tree.contains_key(old_parent));
                assert!(!repair_weight.trees.contains_key(old_parent));
                assert!(!repair_weight.pruned_trees.contains_key(old_parent));
            }
        }
    }

    #[test]
    fn test_find_ancestor_subtree_of_slot() {
        let (blockstore, _, mut repair_weight) = setup_orphan_repair_weight();

        // Ancestor of slot 4 is slot 2, with an existing subtree rooted at 0
        // because there wass a vote for a descendant
        assert_eq!(
            repair_weight.find_ancestor_subtree_of_slot(&blockstore, 4),
            (VecDeque::from([2]), Some(TreeRoot::Root(0)))
        );

        // Ancestors of 5 are [1, 3], with an existing subtree rooted at 0
        // because there wass a vote for a descendant
        assert_eq!(
            repair_weight.find_ancestor_subtree_of_slot(&blockstore, 5),
            (VecDeque::from([1, 3]), Some(TreeRoot::Root(0)))
        );

        // Ancestors of slot 23 are [20, 22], with an existing subtree of 20
        // because there wass a vote for 20
        assert_eq!(
            repair_weight.find_ancestor_subtree_of_slot(&blockstore, 23),
            (VecDeque::from([20, 22]), Some(TreeRoot::Root(20)))
        );

        // Add 22 to `pruned_trees`, ancestor search should now
        // chain it back to 20
        repair_weight.remove_tree(TreeRoot::Root(20)).unwrap();
        repair_weight.insert_new_pruned_tree(20);
        assert_eq!(
            repair_weight.find_ancestor_subtree_of_slot(&blockstore, 23),
            (VecDeque::from([20, 22]), Some(TreeRoot::PrunedRoot(20)))
        );

        // Ancestors of slot 31 are [30], with no existing subtree
        blockstore.add_tree(tr(30) / (tr(31)), true, true, 2, Hash::default());
        assert_eq!(
            repair_weight.find_ancestor_subtree_of_slot(&blockstore, 31),
            (VecDeque::from([30]), None),
        );

        // Set a root at 5
        repair_weight.set_root(5);

        // Ancestor of slot 6 shouldn't include anything from before the root
        // at slot 5
        assert_eq!(
            repair_weight.find_ancestor_subtree_of_slot(&blockstore, 6),
            (VecDeque::from([5]), Some(TreeRoot::Root(5)))
        );

        // Chain orphan 8 back to slot 4 on a different fork
        // Will still include a slot (4) < root, but only one such slot
        blockstore.add_tree(tr(4) / (tr(8)), true, true, 2, Hash::default());
        assert_eq!(
            repair_weight.find_ancestor_subtree_of_slot(&blockstore, 8),
            (VecDeque::from([4]), None),
        );

        // Don't skip, even though 8's chain info is present in blockstore since the merge hasn't
        // happened stop there
        blockstore.add_tree(tr(8) / (tr(40) / tr(41)), true, true, 2, Hash::default());
        assert_eq!(
            repair_weight.find_ancestor_subtree_of_slot(&blockstore, 41),
            (VecDeque::from([8, 40]), Some(TreeRoot::Root(8))),
        )
    }

    #[test]
    fn test_split_off_copy_weight() {
        let (blockstore, _, mut repair_weight) = setup_orphan_repair_weight();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(1, stake);
        repair_weight.add_votes(
            &blockstore,
            vec![(6, vote_pubkeys)].into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Simulate dump from replay
        blockstore.clear_unconfirmed_slot(3);
        repair_weight.split_off(3);
        blockstore.clear_unconfirmed_slot(10);
        repair_weight.split_off(10);

        // Verify orphans
        let mut orphans = repair_weight.trees.keys().copied().collect_vec();
        orphans.sort();
        assert_eq!(vec![0, 3, 8, 10, 20], orphans);

        // Verify weighting
        assert_eq!(
            0,
            repair_weight
                .trees
                .get(&8)
                .unwrap()
                .stake_voted_subtree(&(8, Hash::default()))
                .unwrap()
        );
        assert_eq!(
            stake,
            repair_weight
                .trees
                .get(&3)
                .unwrap()
                .stake_voted_subtree(&(3, Hash::default()))
                .unwrap()
        );
        assert_eq!(
            2 * stake,
            repair_weight
                .trees
                .get(&10)
                .unwrap()
                .stake_voted_subtree(&(10, Hash::default()))
                .unwrap()
        );

        // Get best orphans works as usual
        let mut repairs = vec![];
        let mut processed_slots = vec![repair_weight.root].into_iter().collect();
        repair_weight.get_best_orphans(
            &blockstore,
            &mut processed_slots,
            &mut repairs,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
            4,
        );
        assert_eq!(repairs.len(), 4);
        assert_eq!(repairs[0].slot(), 10);
        assert_eq!(repairs[1].slot(), 20);
        assert_eq!(repairs[2].slot(), 3);
        assert_eq!(repairs[3].slot(), 8);
    }

    #[test]
    fn test_split_off_multi_dump_repair() {
        let blockstore = setup_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(1, stake);
        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            vec![(6, vote_pubkeys)].into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Simulate multiple dumps (whole branch is duplicate) from replay
        blockstore.clear_unconfirmed_slot(3);
        repair_weight.split_off(3);
        blockstore.clear_unconfirmed_slot(5);
        repair_weight.split_off(5);
        blockstore.clear_unconfirmed_slot(6);
        repair_weight.split_off(6);

        // Verify orphans
        let mut orphans = repair_weight.trees.keys().copied().collect_vec();
        orphans.sort();
        assert_eq!(vec![0, 3, 5, 6], orphans);

        // Get best orphans works as usual
        let mut repairs = vec![];
        let mut processed_slots = vec![repair_weight.root].into_iter().collect();
        repair_weight.get_best_orphans(
            &blockstore,
            &mut processed_slots,
            &mut repairs,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
            4,
        );
        assert_eq!(repairs.len(), 3);
        assert_eq!(repairs[0].slot(), 6);
        assert_eq!(repairs[1].slot(), 3);
        assert_eq!(repairs[2].slot(), 5);

        // Simulate repair on 6 and 5
        for (shreds, _) in make_chaining_slot_entries(&[5, 6], 100) {
            blockstore.insert_shreds(shreds, None, true).unwrap();
        }

        // Verify orphans properly updated and chained
        let mut repairs = vec![];
        let mut processed_slots = vec![repair_weight.root].into_iter().collect();
        repair_weight.get_best_orphans(
            &blockstore,
            &mut processed_slots,
            &mut repairs,
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
            4,
        );
        assert_eq!(repairs.len(), 1);
        assert_eq!(repairs[0].slot(), 3);

        let mut orphans = repair_weight.trees.keys().copied().collect_vec();
        orphans.sort();
        assert_eq!(orphans, vec![0, 3]);
    }

    #[test]
    fn test_get_popular_pruned_forks() {
        let blockstore = setup_big_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(10, stake);
        let epoch_stakes = bank.epoch_stakes_map();
        let epoch_schedule = bank.epoch_schedule();

        // Add a little stake for each fork
        let votes = vec![
            (4, vec![vote_pubkeys[0]]),
            (11, vec![vote_pubkeys[1]]),
            (6, vec![vote_pubkeys[2]]),
            (23, vec![vote_pubkeys[3]]),
        ];
        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Set root to 4, there should now be 3 pruned trees with `stake`
        repair_weight.set_root(4);
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.pruned_trees.len(), 3);
        assert!(repair_weight
            .pruned_trees
            .iter()
            .all(
                |(root, pruned_tree)| pruned_tree.stake_voted_subtree(&(*root, Hash::default()))
                    == Some(stake)
            ));

        // No fork has DUPLICATE_THRESHOLD, should not be any popular forks
        assert!(repair_weight
            .get_popular_pruned_forks(epoch_stakes, epoch_schedule)
            .is_empty());

        // 500 stake, still less than DUPLICATE_THRESHOLD, should not be any popular forks
        let five_votes = vote_pubkeys.iter().copied().take(5).collect_vec();
        let votes = vec![(11, five_votes.clone()), (6, five_votes)];
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert!(repair_weight
            .get_popular_pruned_forks(epoch_stakes, epoch_schedule)
            .is_empty());

        // 600 stake, since we voted for leaf, leaf should be returned
        let votes = vec![(11, vec![vote_pubkeys[5]]), (6, vec![vote_pubkeys[6]])];
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(
            vec![6, 11],
            repair_weight
                .get_popular_pruned_forks(epoch_stakes, epoch_schedule)
                .into_iter()
                .sorted()
                .collect_vec()
        );

        // For the last pruned tree we leave 100 stake on 23 and 22 and put 600 stake on 20. We
        // should return 20 and not traverse the tree deeper
        let six_votes = vote_pubkeys.iter().copied().take(6).collect_vec();
        let votes = vec![(20, six_votes)];
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(
            vec![6, 11, 20],
            repair_weight
                .get_popular_pruned_forks(epoch_stakes, epoch_schedule)
                .into_iter()
                .sorted()
                .collect_vec()
        );
    }

    #[test]
    fn test_get_popular_pruned_forks_forks() {
        let blockstore = setup_big_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(10, stake);
        let epoch_stakes = bank.epoch_stakes_map();
        let epoch_schedule = bank.epoch_schedule();

        // Add a little stake for each fork
        let votes = vec![
            (4, vec![vote_pubkeys[0]]),
            (11, vec![vote_pubkeys[1]]),
            (6, vec![vote_pubkeys[2]]),
            (23, vec![vote_pubkeys[3]]),
        ];
        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Prune the entire tree
        std::mem::swap(&mut repair_weight.trees, &mut repair_weight.pruned_trees);
        repair_weight
            .slot_to_tree
            .iter_mut()
            .for_each(|(_, s)| *s = TreeRoot::PrunedRoot(s.slot()));

        // Traverse to 20
        let mut repair_weight_20 = repair_weight.clone();
        repair_weight_20.add_votes(
            &blockstore,
            vec![(20, vote_pubkeys.clone())].into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(
            vec![20],
            repair_weight_20.get_popular_pruned_forks(epoch_stakes, epoch_schedule)
        );

        // 4 and 8 individually do not have enough stake, but 2 is popular
        let votes = vec![(10, vote_pubkeys.iter().copied().skip(6).collect_vec())];
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(
            vec![2],
            repair_weight.get_popular_pruned_forks(epoch_stakes, epoch_schedule)
        );
    }

    #[test]
    fn test_get_popular_pruned_forks_stake_change_across_epoch_boundary() {
        let blockstore = setup_big_forks();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(10, stake);
        let mut epoch_stakes = bank.epoch_stakes_map().clone();
        let mut epoch_schedule = *bank.epoch_schedule();

        // Simulate epoch boundary at slot 10, where half of the stake deactivates
        // Additional epoch boundary at slot 20, where 30% of the stake reactivates
        let initial_stakes = epoch_stakes
            .get(&epoch_schedule.get_epoch(0))
            .unwrap()
            .clone();
        let mut dec_stakes = epoch_stakes
            .get(&epoch_schedule.get_epoch(0))
            .unwrap()
            .clone();
        let mut inc_stakes = epoch_stakes
            .get(&epoch_schedule.get_epoch(0))
            .unwrap()
            .clone();
        epoch_schedule.first_normal_slot = 0;
        epoch_schedule.slots_per_epoch = 10;
        assert_eq!(
            epoch_schedule.get_epoch(10),
            epoch_schedule.get_epoch(9) + 1
        );
        assert_eq!(
            epoch_schedule.get_epoch(20),
            epoch_schedule.get_epoch(19) + 1
        );
        dec_stakes.set_total_stake(dec_stakes.total_stake() - 5 * stake);
        inc_stakes.set_total_stake(dec_stakes.total_stake() + 3 * stake);
        epoch_stakes.insert(epoch_schedule.get_epoch(0), initial_stakes);
        epoch_stakes.insert(epoch_schedule.get_epoch(10), dec_stakes);
        epoch_stakes.insert(epoch_schedule.get_epoch(20), inc_stakes);

        // Add a little stake for each fork
        let votes = vec![
            (4, vec![vote_pubkeys[0]]),
            (11, vec![vote_pubkeys[1]]),
            (6, vec![vote_pubkeys[2]]),
            (23, vec![vote_pubkeys[3]]),
        ];
        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        // Set root to 4, there should now be 3 pruned trees with `stake`
        repair_weight.set_root(4);
        assert_eq!(repair_weight.trees.len(), 1);
        assert_eq!(repair_weight.pruned_trees.len(), 3);
        assert!(repair_weight
            .pruned_trees
            .iter()
            .all(
                |(root, pruned_tree)| pruned_tree.stake_voted_subtree(&(*root, Hash::default()))
                    == Some(stake)
            ));

        // No fork hash `DUPLICATE_THRESHOLD`, should not be any popular forks
        assert!(repair_weight
            .get_popular_pruned_forks(&epoch_stakes, &epoch_schedule)
            .is_empty());

        // 400 stake, For the 6 tree it will be less than `DUPLICATE_THRESHOLD`, however 11
        // has epoch modifications where at some point 400 stake is enough. For 22, although it
        // does cross the second epoch where the stake requirement was less, because it doesn't
        // have any blocks in that epoch the minimum total stake is still 800 which fails.
        let four_votes = vote_pubkeys.iter().copied().take(4).collect_vec();
        let votes = vec![
            (11, four_votes.clone()),
            (6, four_votes.clone()),
            (22, four_votes),
        ];
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );
        assert_eq!(
            vec![11],
            repair_weight.get_popular_pruned_forks(&epoch_stakes, &epoch_schedule)
        );
    }

    fn setup_orphan_repair_weight() -> (Blockstore, Bank, RepairWeight) {
        let blockstore = setup_orphans();
        let stake = 100;
        let (bank, vote_pubkeys) = bank_utils::setup_bank_and_vote_pubkeys_for_tests(2, stake);

        // Add votes for the main fork and orphan forks
        let votes = vec![
            (4, vote_pubkeys.clone()),
            (8, vote_pubkeys.clone()),
            (10, vote_pubkeys.clone()),
            (20, vote_pubkeys),
        ];

        let mut repair_weight = RepairWeight::new(0);
        repair_weight.add_votes(
            &blockstore,
            votes.into_iter(),
            bank.epoch_stakes_map(),
            bank.epoch_schedule(),
        );

        assert!(repair_weight.slot_to_tree.contains_key(&0));

        // Check orphans are present
        for orphan in &[8, 20] {
            assert_eq!(
                repair_weight.trees.get(orphan).unwrap().tree_root().0,
                *orphan
            );
            assert_eq!(
                *repair_weight.slot_to_tree.get(orphan).unwrap(),
                TreeRoot::Root(*orphan)
            );
        }
        (blockstore, bank, repair_weight)
    }

    fn check_old_root_purged_verify_new_root(
        old_root: Slot,
        new_root: Slot,
        repair_weight: &RepairWeight,
    ) {
        // Check old root is purged
        assert!(!repair_weight.trees.contains_key(&old_root));
        assert!(!repair_weight.slot_to_tree.contains_key(&old_root));

        // Validate new root
        assert_eq!(
            repair_weight.trees.get(&new_root).unwrap().tree_root().0,
            new_root
        );
        assert_eq!(
            *repair_weight.slot_to_tree.get(&new_root).unwrap(),
            TreeRoot::Root(new_root)
        );
        assert_eq!(repair_weight.root, new_root);
    }

    fn setup_orphans() -> Blockstore {
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

            Orphans:
               slot 8
                  |
               slot 10
                  |
               slot 11

            Orphans:
              slot 20
                 |
              slot 22
                 |
              slot 23
        */

        let blockstore = setup_forks();
        blockstore.add_tree(tr(8) / (tr(10) / (tr(11))), true, true, 2, Hash::default());
        blockstore.add_tree(tr(20) / (tr(22) / (tr(23))), true, true, 2, Hash::default());
        assert!(blockstore.orphan(8).unwrap().is_some());
        blockstore
    }

    fn setup_big_forks() -> Blockstore {
        /*
            Build fork structure:
                      slot 0
                        |
                      slot 1
                      /    \
                /----|      |----|
            slot 2               |
            /    \            slot 3
        slot 4  slot 8      /        \
                   |     slot 5    slot 9
                slot 10     |         |
                   |     slot 6    slot 20
                slot 11               |
                                   slot 22
                                      |
                                   slot 23
        */
        let blockstore = setup_orphans();
        // Connect orphans to main fork
        blockstore.add_tree(tr(2) / tr(8), true, true, 2, Hash::default());
        blockstore.add_tree(tr(3) / (tr(9) / tr(20)), true, true, 2, Hash::default());
        blockstore
    }

    fn setup_forks() -> Blockstore {
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
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&ledger_path).unwrap();
        blockstore.add_tree(forks, false, true, 2, Hash::default());
        blockstore
    }
}

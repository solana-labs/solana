use {
    solana_runtime::bank::Bank,
    solana_sdk::{hash::Hash, pubkey::Pubkey, slot_history::Slot},
    std::{
        collections::{HashMap, HashSet},
        sync::{Arc, RwLock},
    },
};

#[derive(Default)]
pub struct VoteStakeTracker {
    voted: HashSet<Pubkey>,
    stake: u64,
}

impl VoteStakeTracker {
    // Returns tuple (reached_threshold_results, is_new) where
    // Each index in `reached_threshold_results` is true if the corresponding threshold in the input
    // `thresholds_to_check` was newly reached by adding the stake of the input `vote_pubkey`
    // `is_new` is true if the vote has not been seen before
    pub fn add_vote_pubkey(
        &mut self,
        vote_pubkey: Pubkey,
        stake: u64,
        total_stake: u64,
        thresholds_to_check: &[f64],
    ) -> (Vec<bool>, bool) {
        let is_new = !self.voted.contains(&vote_pubkey);
        if is_new {
            self.voted.insert(vote_pubkey);
            let old_stake = self.stake;
            let new_stake = self.stake + stake;
            self.stake = new_stake;
            let reached_threshold_results: Vec<bool> = thresholds_to_check
                .iter()
                .map(|threshold| {
                    let threshold_stake = (total_stake as f64 * threshold) as u64;
                    old_stake <= threshold_stake && threshold_stake < new_stake
                })
                .collect();
            (reached_threshold_results, is_new)
        } else {
            (vec![false; thresholds_to_check.len()], is_new)
        }
    }

    pub fn voted(&self) -> &HashSet<Pubkey> {
        &self.voted
    }

    pub fn stake(&self) -> u64 {
        self.stake
    }
}

#[derive(Default)]
pub struct SlotVoteTracker {
    // Maps pubkeys that have voted for this slot
    // to whether or not we've seen the vote on gossip.
    // True if seen on gossip, false if only seen in replay.
    pub voted: HashMap<Pubkey, bool>,
    optimistic_votes_tracker: HashMap<Hash, VoteStakeTracker>,
    pub voted_slot_updates: Option<Vec<Pubkey>>,
    pub gossip_only_stake: u64,
}

impl SlotVoteTracker {
    pub fn get_voted_slot_updates(&mut self) -> Option<Vec<Pubkey>> {
        self.voted_slot_updates.take()
    }

    pub fn get_or_insert_optimistic_votes_tracker(&mut self, hash: Hash) -> &mut VoteStakeTracker {
        self.optimistic_votes_tracker.entry(hash).or_default()
    }
    pub fn optimistic_votes_tracker(&self, hash: &Hash) -> Option<&VoteStakeTracker> {
        self.optimistic_votes_tracker.get(hash)
    }
}

#[derive(Default)]
pub struct VoteTracker {
    // Map from a slot to a set of validators who have voted for that slot
    slot_vote_trackers: RwLock<HashMap<Slot, Arc<RwLock<SlotVoteTracker>>>>,
}

impl VoteTracker {
    pub fn get_or_insert_slot_tracker(&self, slot: Slot) -> Arc<RwLock<SlotVoteTracker>> {
        if let Some(slot_vote_tracker) = self.slot_vote_trackers.read().unwrap().get(&slot) {
            return slot_vote_tracker.clone();
        }
        let mut slot_vote_trackers = self.slot_vote_trackers.write().unwrap();
        slot_vote_trackers.entry(slot).or_default().clone()
    }

    pub fn get_slot_vote_tracker(&self, slot: Slot) -> Option<Arc<RwLock<SlotVoteTracker>>> {
        self.slot_vote_trackers.read().unwrap().get(&slot).cloned()
    }

    /// pub for tests
    pub fn insert_vote(&self, slot: Slot, pubkey: Pubkey) {
        let mut w_slot_vote_trackers = self.slot_vote_trackers.write().unwrap();

        let slot_vote_tracker = w_slot_vote_trackers.entry(slot).or_default();

        let mut w_slot_vote_tracker = slot_vote_tracker.write().unwrap();

        w_slot_vote_tracker.voted.insert(pubkey, true);
        if let Some(ref mut voted_slot_updates) = w_slot_vote_tracker.voted_slot_updates {
            voted_slot_updates.push(pubkey)
        } else {
            w_slot_vote_tracker.voted_slot_updates = Some(vec![pubkey]);
        }
    }

    fn purge_stale_state(&self, root_bank: &Bank) {
        // Purge any outdated slot data
        let new_root = root_bank.slot();
        self.slot_vote_trackers
            .write()
            .unwrap()
            .retain(|slot, _| *slot >= new_root);
    }

    pub fn progress_with_new_root_bank(&self, root_bank: &Bank) {
        self.purge_stale_state(root_bank);
    }

    /// pub for tests
    pub fn num_trackers(&self) -> usize {
        self.slot_vote_trackers.read().unwrap().len()
    }
}

#[cfg(test)]
mod test {
    use {super::*, solana_runtime::commitment::VOTE_THRESHOLD_SIZE};

    #[test]
    fn test_add_vote_pubkey() {
        let total_epoch_stake = 10;
        let mut vote_stake_tracker = VoteStakeTracker::default();
        for i in 0..10 {
            let pubkey = solana_sdk::pubkey::new_rand();
            let (is_confirmed_thresholds, is_new) = vote_stake_tracker.add_vote_pubkey(
                pubkey,
                1,
                total_epoch_stake,
                &[VOTE_THRESHOLD_SIZE, 0.0],
            );
            let stake = vote_stake_tracker.stake();
            let (is_confirmed_thresholds2, is_new2) = vote_stake_tracker.add_vote_pubkey(
                pubkey,
                1,
                total_epoch_stake,
                &[VOTE_THRESHOLD_SIZE, 0.0],
            );
            let stake2 = vote_stake_tracker.stake();

            // Stake should not change from adding same pubkey twice
            assert_eq!(stake, stake2);
            assert!(!is_confirmed_thresholds2[0]);
            assert!(!is_confirmed_thresholds2[1]);
            assert!(!is_new2);
            assert_eq!(is_confirmed_thresholds.len(), 2);
            assert_eq!(is_confirmed_thresholds2.len(), 2);

            // at i == 6, the voted stake is 70%, which is the first time crossing
            // the supermajority threshold
            if i == 6 {
                assert!(is_confirmed_thresholds[0]);
            } else {
                assert!(!is_confirmed_thresholds[0]);
            }

            // at i == 6, the voted stake is 10%, which is the first time crossing
            // the 0% threshold
            if i == 0 {
                assert!(is_confirmed_thresholds[1]);
            } else {
                assert!(!is_confirmed_thresholds[1]);
            }
            assert!(is_new);
        }
    }
}

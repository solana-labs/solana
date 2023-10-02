use {
    crate::{
        fork_choice::ForkChoice,
        heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
        progress_map::{ForkProgress, ProgressMap},
    },
    solana_runtime::bank::Bank,
    solana_sdk::{hash::Hash, pubkey::Pubkey, slot_history::Slot},
    std::{
        collections::{HashMap, HashSet},
        sync::Arc,
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

pub fn initialize_progress_and_fork_choice(
    root_bank: &Bank,
    mut frozen_banks: Vec<Arc<Bank>>,
    my_pubkey: &Pubkey,
    vote_account: &Pubkey,
    duplicate_slot_hashes: Vec<(Slot, Hash)>,
) -> (ProgressMap, HeaviestSubtreeForkChoice) {
    let mut progress = ProgressMap::default();

    frozen_banks.sort_by_key(|bank| bank.slot());

    // Initialize progress map with any root banks
    for bank in &frozen_banks {
        let prev_leader_slot = progress.get_bank_prev_leader_slot(bank);
        progress.insert(
            bank.slot(),
            ForkProgress::new_from_bank(bank, my_pubkey, vote_account, prev_leader_slot, 0, 0),
        );
    }
    let root = root_bank.slot();
    let mut heaviest_subtree_fork_choice =
        HeaviestSubtreeForkChoice::new_from_frozen_banks((root, root_bank.hash()), &frozen_banks);

    for slot_hash in duplicate_slot_hashes {
        heaviest_subtree_fork_choice.mark_fork_invalid_candidate(&slot_hash);
    }

    (progress, heaviest_subtree_fork_choice)
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

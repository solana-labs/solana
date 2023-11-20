use {
    crate::cluster_info_vote_listener::Threshold, solana_sdk::pubkey::Pubkey,
    std::collections::HashSet,
};

#[derive(Default)]
pub struct VoteStakeTracker {
    voted: HashSet<Pubkey>,
    stake: u64,
}

impl VoteStakeTracker {
    // Returns tuple (reached_threshold_results, is_new) where
    // `threshold` is in `reached_threshold_results` if `threshold` was present in
    // `thresholds_to_check` and newly reached by adding the stake of the input `vote_pubkey`
    // `is_new` is true if the vote has not been seen before
    pub fn add_vote_pubkey<I>(
        &mut self,
        vote_pubkey: Pubkey,
        stake: u64,
        total_stake: u64,
        thresholds_to_check: I,
    ) -> (HashSet<Threshold>, bool)
    where
        I: Iterator<Item = Threshold>,
    {
        let is_new = !self.voted.contains(&vote_pubkey);
        if is_new {
            self.voted.insert(vote_pubkey);
            let old_stake = self.stake;
            let new_stake = self.stake + stake;
            self.stake = new_stake;
            let reached_threshold_results = thresholds_to_check
                .filter(|threshold| {
                    let threshold_stake = (total_stake as f64 * threshold.threshold()) as u64;
                    old_stake <= threshold_stake && threshold_stake < new_stake
                })
                .collect();
            (reached_threshold_results, is_new)
        } else {
            (HashSet::default(), is_new)
        }
    }

    pub fn voted(&self) -> &HashSet<Pubkey> {
        &self.voted
    }

    pub fn stake(&self) -> u64 {
        self.stake
    }
}

#[cfg(test)]
mod test {
    use {super::*, crate::cluster_info_vote_listener::Threshold::*};

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
                [OptimisticThreshold, ZeroThreshold].into_iter(),
            );
            let stake = vote_stake_tracker.stake();
            let (is_confirmed_thresholds2, is_new2) = vote_stake_tracker.add_vote_pubkey(
                pubkey,
                1,
                total_epoch_stake,
                [OptimisticThreshold, ZeroThreshold].into_iter(),
            );
            let stake2 = vote_stake_tracker.stake();

            // Stake should not change from adding same pubkey twice
            assert_eq!(stake, stake2);
            assert!(is_confirmed_thresholds2.is_empty());
            assert!(!is_new2);

            // at i == 6, the voted stake is 70%, which is the first time crossing
            // the supermajority threshold
            if i == 6 {
                assert!(is_confirmed_thresholds.contains(&OptimisticThreshold));
            } else {
                assert!(!is_confirmed_thresholds.contains(&OptimisticThreshold));
            }

            // at i == 6, the voted stake is 10%, which is the first time crossing
            // the 0% threshold
            if i == 0 {
                assert!(is_confirmed_thresholds.contains(&ZeroThreshold));
            } else {
                assert!(!is_confirmed_thresholds.contains(&ZeroThreshold));
            }
            assert!(is_new);
        }
    }
}

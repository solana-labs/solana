use {solana_sdk::pubkey::Pubkey, std::collections::HashSet};

#[derive(Default)]
pub struct VoteThresholdCheckResult {
    pub check_duplicate: bool,
    pub check_vote: bool,
    pub is_new: bool,
}
#[derive(Default)]
pub struct VoteStakeTracker {
    voted: HashSet<Pubkey>,
    stake: u64,
}

impl VoteStakeTracker {
    // Returns VoteThresholdCheckResult.
    // It checks both DUPLICATE_THRESHOLD and VOTE_THRESHOLD_SIZE by adding the
    // stake of the vote from the new `vote_pubkey` and comparing against the target
    // threshold.
    // `is_new` is true if the vote_pubkey has not been seen before.
    pub fn add_vote_pubkey(
        &mut self,
        vote_pubkey: Pubkey,
        stake: u64,
        total_stake: u64,
        thresholds_to_check: &[f64],
    ) -> VoteThresholdCheckResult {
        if self.voted.insert(vote_pubkey) {
            // A new vote that we haven't seen before.
            let old_stake = self.stake;
            let new_stake = self.stake + stake;
            self.stake = new_stake;
            let check = |threshold| {
                let threshold_stake = (total_stake as f64 * threshold) as u64;
                old_stake <= threshold_stake && threshold_stake < new_stake
            };
            VoteThresholdCheckResult {
                check_duplicate: check(thresholds_to_check[0]),
                check_vote: check(thresholds_to_check[1]),
                is_new: true,
            }
        } else {
            VoteThresholdCheckResult::default()
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
    use {super::*, solana_runtime::commitment::VOTE_THRESHOLD_SIZE};

    #[test]
    fn test_add_vote_pubkey() {
        let total_epoch_stake = 10;
        let mut vote_stake_tracker = VoteStakeTracker::default();
        for i in 0..10 {
            let pubkey = solana_sdk::pubkey::new_rand();
            let VoteThresholdCheckResult {
                check_duplicate: duplicate_check,
                check_vote: vote_check,
                is_new,
            } = vote_stake_tracker.add_vote_pubkey(
                pubkey,
                1,
                total_epoch_stake,
                &[VOTE_THRESHOLD_SIZE, 0.0],
            );
            let stake = vote_stake_tracker.stake();
            let VoteThresholdCheckResult {
                check_duplicate: duplicate_check2,
                check_vote: vote_check2,
                is_new: is_new2,
            } = vote_stake_tracker.add_vote_pubkey(
                pubkey,
                1,
                total_epoch_stake,
                &[VOTE_THRESHOLD_SIZE, 0.0],
            );
            let stake2 = vote_stake_tracker.stake();

            // Stake should not change from adding same pubkey twice
            assert_eq!(stake, stake2);
            assert!(!duplicate_check2);
            assert!(!vote_check2);
            assert!(!is_new2);

            // at i == 6, the voted stake is 70%, which is the first time crossing
            // the supermajority threshold
            assert!((i == 6 && duplicate_check) || ((i != 6) && !(duplicate_check)));

            // at i == 0, the voted stake is 10%, which is the first time crossing
            // the 0% threshold
            assert!((i == 0 && vote_check) || (i != 0 && !vote_check));
            assert!(is_new);
        }
    }
}

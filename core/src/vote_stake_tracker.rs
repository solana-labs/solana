use solana_runtime::commitment::VOTE_THRESHOLD_SIZE;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;

#[derive(Default)]
pub struct VoteStakeTracker {
    voted: HashSet<Pubkey>,
    stake: u64,
}

impl VoteStakeTracker {
    // Returns tuple (is_confirmed, is_new) where
    // `is_confirmed` is true if the stake that has voted has just crosssed the supermajority
    // of stake
    // `is_new` is true if the vote has not been seen before
    pub fn add_vote_pubkey(
        &mut self,
        vote_pubkey: Pubkey,
        stake: u64,
        total_stake: u64,
    ) -> (bool, bool) {
        let is_new = !self.voted.contains(&vote_pubkey);
        if is_new {
            self.voted.insert(vote_pubkey);
            let supermajority_stake = (total_stake as f64 * VOTE_THRESHOLD_SIZE) as u64;
            let old_stake = self.stake;
            let new_stake = self.stake + stake;
            self.stake = new_stake;
            (
                old_stake <= supermajority_stake && supermajority_stake < new_stake,
                is_new,
            )
        } else {
            (false, is_new)
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
    use super::*;

    #[test]
    fn test_add_vote_pubkey() {
        let total_epoch_stake = 10;
        let mut vote_stake_tracker = VoteStakeTracker::default();
        for i in 0..10 {
            let pubkey = solana_sdk::pubkey::new_rand();
            let (is_confirmed, is_new) =
                vote_stake_tracker.add_vote_pubkey(pubkey, 1, total_epoch_stake);
            let stake = vote_stake_tracker.stake();
            let (is_confirmed2, is_new2) =
                vote_stake_tracker.add_vote_pubkey(pubkey, 1, total_epoch_stake);
            let stake2 = vote_stake_tracker.stake();

            // Stake should not change from adding same pubkey twice
            assert_eq!(stake, stake2);
            assert!(!is_confirmed2);
            assert!(!is_new2);

            // at i == 6, the voted stake is 70%, which is the first time crossing
            // the supermajority threshold
            if i == 6 {
                assert!(is_confirmed);
            } else {
                assert!(!is_confirmed);
            }
            assert!(is_new);
        }
    }
}

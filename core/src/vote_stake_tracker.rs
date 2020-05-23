use crate::consensus::VOTE_THRESHOLD_SIZE;
use solana_sdk::pubkey::Pubkey;
use std::{collections::HashSet, sync::Arc};

#[derive(Default)]
pub struct VoteStakeTracker {
    voted: HashSet<Arc<Pubkey>>,
    stake: u64,
}

impl VoteStakeTracker {
    // Returns true if the stake that has voted has just crosssed the supermajority
    // of stake
    pub fn add_vote_pubkey(
        &mut self,
        vote_pubkey: Arc<Pubkey>,
        stake: u64,
        total_epoch_stake: u64,
    ) -> bool {
        if !self.voted.contains(&vote_pubkey) {
            self.voted.insert(vote_pubkey);
            let ratio_before = self.stake as f64 / total_epoch_stake as f64;
            self.stake += stake;
            let ratio_now = self.stake as f64 / total_epoch_stake as f64;

            ratio_before <= VOTE_THRESHOLD_SIZE && ratio_now > VOTE_THRESHOLD_SIZE
        } else {
            false
        }
    }

    pub fn voted(&self) -> &HashSet<Arc<Pubkey>> {
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
            let pubkey = Arc::new(Pubkey::new_rand());
            let res = vote_stake_tracker.add_vote_pubkey(pubkey.clone(), 1, total_epoch_stake);
            let stake = vote_stake_tracker.stake();
            vote_stake_tracker.add_vote_pubkey(pubkey.clone(), 1, total_epoch_stake);
            let stake2 = vote_stake_tracker.stake();

            // Stake should not change from adding same pubkey twice
            assert_eq!(stake, stake2);

            // at i == 7, the voted stake is 70%, which is the first time crossing
            // the supermajority threshold
            if i == 6 {
                assert!(res);
            } else {
                assert!(!res);
            }
        }
    }
}

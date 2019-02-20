use crate::leader_schedule::LeaderSchedule;
use hashbrown::{HashMap, HashSet};
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::vote_program::VoteState;

// Return true of the latest vote is between the lower and upper bounds (inclusive)
fn is_active_staker(vote_state: &VoteState, lower_bound: u64, upper_bound: u64) -> bool {
    vote_state
        .votes
        .back()
        .filter(|vote| vote.tick_height >= lower_bound && vote.tick_height <= upper_bound)
        .is_some()
}

/// The set of stakers that have voted near the time of construction
pub struct ActiveStakers {
    stakes: HashMap<Pubkey, u64>,
}

impl ActiveStakers {
    pub fn new_with_upper_bound(bank: &Bank, lower_bound: u64, upper_bound: u64) -> Self {
        let stakes = bank
            .vote_states(|vote_state| is_active_staker(vote_state, lower_bound, upper_bound))
            .iter()
            .filter_map(|vote_state| {
                let pubkey = vote_state.staker_id;
                let stake = bank.get_balance(&pubkey);
                if stake > 0 {
                    Some((pubkey, stake))
                } else {
                    None
                }
            })
            .collect();
        Self { stakes }
    }

    pub fn new(bank: &Bank, lower_bound: u64) -> Self {
        Self::new_with_upper_bound(bank, lower_bound, bank.tick_height())
    }

    /// Return a map from staker pubkeys to their respective stakes.
    pub fn stakes(&self) -> HashMap<Pubkey, u64> {
        self.stakes.clone()
    }

    /// Return the pubkeys of each staker.
    pub fn stakers(&self) -> HashSet<Pubkey> {
        self.stakes.keys().cloned().collect()
    }

    pub fn leader_schedule(&self) -> LeaderSchedule {
        let mut stakers: Vec<_> = self.stakes.keys().cloned().collect();
        stakers.sort();
        LeaderSchedule::new(stakers)
    }
}

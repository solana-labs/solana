use crate::leader_schedule::LeaderSchedule;
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

fn rank_stakes(stakes: &mut Vec<(Pubkey, u64)>) {
    // Rank first by stake. If stakes are the same we rank by pubkey to ensure a
    // deterministic result.
    // Note: Use unstable sort, because we dedup right after to remove the equal elements.
    stakes.sort_unstable_by(|(pubkey0, stake0), (pubkey1, stake1)| {
        if stake0 == stake1 {
            pubkey0.cmp(&pubkey1)
        } else {
            stake0.cmp(&stake1)
        }
    });

    // Now that it's sorted, we can do an O(n) dedup.
    stakes.dedup();
}

/// The set of stakers that have voted near the time of construction
pub struct ActiveStakers {
    stakes: Vec<(Pubkey, u64)>,
}

impl ActiveStakers {
    pub fn new_with_upper_bound(bank: &Bank, lower_bound: u64, upper_bound: u64) -> Self {
        let mut stakes: Vec<_> = bank
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
        rank_stakes(&mut stakes);
        Self { stakes }
    }

    pub fn new(bank: &Bank, lower_bound: u64) -> Self {
        Self::new_with_upper_bound(bank, lower_bound, bank.tick_height())
    }

    /// Return the pubkeys of each staker.
    pub fn pubkeys(&self) -> Vec<Pubkey> {
        self.stakes.iter().map(|(pubkey, _stake)| *pubkey).collect()
    }

    pub fn leader_schedule(&self) -> LeaderSchedule {
        LeaderSchedule::new(self.pubkeys())
    }
}

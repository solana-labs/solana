use crate::leader_schedule::LeaderSchedule;
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::{DEFAULT_SLOTS_PER_EPOCH, DEFAULT_TICKS_PER_SLOT};
use solana_sdk::vote_program::VoteState;

pub const DEFAULT_ACTIVE_WINDOW_TICK_LENGTH: u64 = DEFAULT_SLOTS_PER_EPOCH * DEFAULT_TICKS_PER_SLOT;

// Return true of the latest vote is between the lower and upper bounds (inclusive)
fn is_active_staker(vote_state: &VoteState, lower_bound: u64, upper_bound: u64) -> bool {
    vote_state
        .votes
        .back()
        .filter(|vote| vote.tick_height >= lower_bound && vote.tick_height <= upper_bound)
        .is_some()
}

pub fn rank_stakes(stakes: &mut Vec<(Pubkey, u64)>) {
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
    pub fn new_with_bounds(bank: &Bank, active_window_tick_length: u64, upper_bound: u64) -> Self {
        let lower_bound = upper_bound.saturating_sub(active_window_tick_length);
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

    pub fn new(bank: &Bank) -> Self {
        Self::new_with_bounds(bank, DEFAULT_ACTIVE_WINDOW_TICK_LENGTH, bank.tick_height())
    }

    pub fn ranked_stakes(&self) -> Vec<(Pubkey, u64)> {
        self.stakes.clone()
    }

    /// Return the pubkeys of each staker.
    pub fn pubkeys(&self) -> Vec<Pubkey> {
        self.stakes.iter().map(|(pubkey, _stake)| *pubkey).collect()
    }

    /// Return the sorted pubkeys of each staker. Useful for testing.
    pub fn sorted_pubkeys(&self) -> Vec<Pubkey> {
        let mut pubkeys = self.pubkeys();
        pubkeys.sort_unstable();
        pubkeys
    }

    pub fn leader_schedule(&self) -> LeaderSchedule {
        LeaderSchedule::new(self.pubkeys())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use solana_runtime::bank::Bank;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::vote_transaction::VoteTransaction;

    pub fn new_vote_account(
        from_keypair: &Keypair,
        voting_pubkey: &Pubkey,
        bank: &Bank,
        num_tokens: u64,
    ) {
        let last_id = bank.last_id();
        let tx = VoteTransaction::new_account(from_keypair, *voting_pubkey, last_id, num_tokens, 0);
        bank.process_transaction(&tx).unwrap();
    }

    pub fn push_vote<T: KeypairUtil>(voting_keypair: &T, bank: &Bank, tick_height: u64) {
        let last_id = bank.last_id();
        let tx = VoteTransaction::new_vote(voting_keypair, tick_height, last_id, 0);
        bank.process_transaction(&tx).unwrap();
    }

    pub fn new_vote_account_with_vote<T: KeypairUtil>(
        from_keypair: &Keypair,
        voting_keypair: &T,
        bank: &Bank,
        num_tokens: u64,
        tick_height: u64,
    ) {
        new_vote_account(from_keypair, &voting_keypair.pubkey(), bank, num_tokens);
        push_vote(voting_keypair, bank, tick_height);
    }

    #[test]
    fn test_active_set() {
        solana_logger::setup();

        let leader_id = Keypair::new().pubkey();
        let active_window_tick_length = 1000;
        let (genesis_block, mint_keypair) = GenesisBlock::new_with_leader(10000, leader_id, 500);
        let bank = Bank::new(&genesis_block);

        let bootstrap_ids = vec![genesis_block.bootstrap_leader_id];

        // Insert a bunch of votes at height "start_height"
        let start_height = 3;
        let num_old_ids = 20;
        let mut old_ids = vec![];
        for _ in 0..num_old_ids {
            let new_keypair = Keypair::new();
            let pk = new_keypair.pubkey();
            old_ids.push(pk);

            // Give the account some stake
            bank.transfer(5, &mint_keypair, pk, genesis_block.last_id())
                .unwrap();

            // Create a vote account and push a vote
            new_vote_account_with_vote(&new_keypair, &Keypair::new(), &bank, 1, start_height);
        }
        old_ids.sort();

        // Insert a bunch of votes at height "start_height + active_window_tick_length"
        let num_new_ids = 10;
        let mut new_ids = vec![];
        for _ in 0..num_new_ids {
            let new_keypair = Keypair::new();
            let pk = new_keypair.pubkey();
            new_ids.push(pk);
            // Give the account some stake
            bank.transfer(5, &mint_keypair, pk, genesis_block.last_id())
                .unwrap();

            // Create a vote account and push a vote
            let tick_height = start_height + active_window_tick_length + 1;
            new_vote_account_with_vote(&new_keypair, &Keypair::new(), &bank, 1, tick_height);
        }
        new_ids.sort();

        // Query for the active set at various heights
        let result = ActiveStakers::new_with_bounds(&bank, active_window_tick_length, 0).pubkeys();
        assert_eq!(result, bootstrap_ids);

        let result =
            ActiveStakers::new_with_bounds(&bank, active_window_tick_length, start_height - 1)
                .pubkeys();
        assert_eq!(result, bootstrap_ids);

        let result = ActiveStakers::new_with_bounds(
            &bank,
            active_window_tick_length,
            active_window_tick_length + start_height - 1,
        )
        .sorted_pubkeys();
        assert_eq!(result, old_ids);

        let result = ActiveStakers::new_with_bounds(
            &bank,
            active_window_tick_length,
            active_window_tick_length + start_height,
        )
        .sorted_pubkeys();
        assert_eq!(result, old_ids);

        let result = ActiveStakers::new_with_bounds(
            &bank,
            active_window_tick_length,
            active_window_tick_length + start_height + 1,
        )
        .sorted_pubkeys();
        assert_eq!(result, new_ids);

        let result = ActiveStakers::new_with_bounds(
            &bank,
            active_window_tick_length,
            2 * active_window_tick_length + start_height,
        )
        .sorted_pubkeys();
        assert_eq!(result, new_ids);

        let result = ActiveStakers::new_with_bounds(
            &bank,
            active_window_tick_length,
            2 * active_window_tick_length + start_height + 1,
        )
        .sorted_pubkeys();
        assert_eq!(result, new_ids);

        let result = ActiveStakers::new_with_bounds(
            &bank,
            active_window_tick_length,
            2 * active_window_tick_length + start_height + 2,
        )
        .sorted_pubkeys();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_multiple_vote() {
        let leader_keypair = Keypair::new();
        let leader_id = leader_keypair.pubkey();
        let active_window_tick_length = 1000;
        let (genesis_block, _mint_keypair) = GenesisBlock::new_with_leader(10000, leader_id, 500);
        let bank = Bank::new(&genesis_block);

        // Bootstrap leader should be in the active set even without explicit votes
        {
            let result = ActiveStakers::new_with_bounds(&bank, active_window_tick_length, 0)
                .sorted_pubkeys();
            assert_eq!(result, vec![leader_id]);

            let result = ActiveStakers::new_with_bounds(
                &bank,
                active_window_tick_length,
                active_window_tick_length,
            )
            .sorted_pubkeys();
            assert_eq!(result, vec![leader_id]);

            let result = ActiveStakers::new_with_bounds(
                &bank,
                active_window_tick_length,
                active_window_tick_length + 1,
            )
            .sorted_pubkeys();
            assert_eq!(result.len(), 0);
        }

        // Check that a node that votes twice in a row will get included in the active
        // window

        // Create a vote account
        let voting_keypair = Keypair::new();
        new_vote_account_with_vote(&leader_keypair, &voting_keypair, &bank, 1, 1);

        {
            let result = ActiveStakers::new_with_bounds(
                &bank,
                active_window_tick_length,
                active_window_tick_length + 1,
            )
            .sorted_pubkeys();
            assert_eq!(result, vec![leader_id]);

            let result = ActiveStakers::new_with_bounds(
                &bank,
                active_window_tick_length,
                active_window_tick_length + 2,
            )
            .sorted_pubkeys();
            assert_eq!(result.len(), 0);
        }

        // Vote at tick_height 2
        push_vote(&voting_keypair, &bank, 2);

        {
            let result = ActiveStakers::new_with_bounds(
                &bank,
                active_window_tick_length,
                active_window_tick_length + 2,
            )
            .sorted_pubkeys();
            assert_eq!(result, vec![leader_id]);

            let result = ActiveStakers::new_with_bounds(
                &bank,
                active_window_tick_length,
                active_window_tick_length + 3,
            )
            .sorted_pubkeys();
            assert_eq!(result.len(), 0);
        }
    }

    #[test]
    fn test_rank_stakes_basic() {
        let pubkey0 = Keypair::new().pubkey();
        let pubkey1 = Keypair::new().pubkey();
        let mut stakes = vec![(pubkey0, 2), (pubkey1, 1)];
        rank_stakes(&mut stakes);
        assert_eq!(stakes, vec![(pubkey1, 1), (pubkey0, 2)]);
    }

    #[test]
    fn test_rank_stakes_with_dup() {
        let pubkey0 = Keypair::new().pubkey();
        let pubkey1 = Keypair::new().pubkey();
        let mut stakes = vec![(pubkey0, 1), (pubkey1, 2), (pubkey0, 1)];
        rank_stakes(&mut stakes);
        assert_eq!(stakes, vec![(pubkey0, 1), (pubkey1, 2)]);
    }

    #[test]
    fn test_rank_stakes_with_equal_stakes() {
        let pubkey0 = Pubkey::default();
        let pubkey1 = Keypair::new().pubkey();
        let mut stakes = vec![(pubkey0, 1), (pubkey1, 1)];
        rank_stakes(&mut stakes);
        assert_eq!(stakes, vec![(pubkey0, 1), (pubkey1, 1)]);
    }
}

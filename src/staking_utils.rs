use solana_runtime::bank::Bank;

/// Looks through vote accounts, and finds the latest slot that has achieved
/// supermajority lockout
pub fn get_supermajority_slot(bank: &Bank) -> Option<u64> {
    // Find the amount of stake needed for supermajority
    let stakes_and_lockouts = stakes_and_lockouts(bank);
    let total_stake: u64 = stakes_and_lockouts.iter().map(|s| s.0).sum();
    let supermajority_stake = total_stake * 2 / 3;

    // Filter out the states that don't have a max lockout
    find_supermajority_slot(supermajority_stake, &stakes_and_lockouts)
}

fn find_supermajority_slot(
    supermajority_stake: u64,
    stakes_and_lockouts: &[(u64, Option<u64>)],
) -> Option<u64> {
    // Filter out the states that don't have a max lockout
    let mut stakes_and_lockouts: Vec<_> = stakes_and_lockouts
        .iter()
        .filter_map(|(stake, slot)| slot.map(|s| (stake, s)))
        .collect();

    // Sort by the root slot, in descending order
    stakes_and_lockouts.sort_unstable_by(|s1, s2| s1.1.cmp(&s2.1).reverse());

    // Find if any slot has achieved sufficient votes for supermajority lockout
    let mut total = 0;
    for (stake, slot) in stakes_and_lockouts {
        total += stake;
        if total > supermajority_stake {
            return Some(slot);
        }
    }

    None
}

pub fn stakes_and_lockouts(bank: &Bank) -> Vec<(u64, Option<u64>)> {
    let vote_states: Vec<_> = bank.vote_states(|_| true);
    vote_states
        .into_iter()
        .filter_map(|state| {
            let stake = bank.get_balance(&state.staker_id);
            if stake > 0 {
                Some((stake, state.root_slot))
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voting_keypair::tests as voting_keypair_tests;
    use hashbrown::HashSet;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::iter::FromIterator;

    #[test]
    fn test_stakes_and_lockouts() {
        let validator = Keypair::new();
        let voter = Keypair::new();

        let (genesis_block, mint_keypair) = GenesisBlock::new(500);
        let bank = Bank::new(&genesis_block);
        let bank_voter = Keypair::new();

        // Give the validator some stake
        bank.transfer(
            1,
            &mint_keypair,
            validator.pubkey(),
            genesis_block.last_id(),
        )
        .unwrap();

        voting_keypair_tests::new_vote_account_with_vote(&validator, &voter, &bank, 1, 0);
        assert_eq!(bank.get_balance(&validator.pubkey()), 0);
        // Validator has zero balance, so they get filtered out.Only the bootstrap leader
        // created by the genesis block will get included
        assert_eq!(stakes_and_lockouts(&bank), vec![(1, None)]);

        voting_keypair_tests::new_vote_account_with_vote(&mint_keypair, &bank_voter, &bank, 1, 0);

        let result: HashSet<_> = HashSet::from_iter(stakes_and_lockouts(&bank).iter().cloned());
        let expected: HashSet<_> = HashSet::from_iter(vec![(1, None), (498, None)]);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_find_supermajority_slot() {
        let supermajority = 10;

        let stakes_and_slots = vec![];
        assert_eq!(
            find_supermajority_slot(supermajority, &stakes_and_slots),
            None
        );

        let stakes_and_slots = vec![(5, None), (5, None)];
        assert_eq!(
            find_supermajority_slot(supermajority, &stakes_and_slots),
            None
        );

        let stakes_and_slots = vec![(5, None), (5, None), (9, Some(2))];
        assert_eq!(
            find_supermajority_slot(supermajority, &stakes_and_slots),
            None
        );

        let stakes_and_slots = vec![(5, None), (5, None), (9, Some(2)), (1, Some(3))];
        assert_eq!(
            find_supermajority_slot(supermajority, &stakes_and_slots),
            None
        );

        let stakes_and_slots = vec![(5, None), (5, None), (9, Some(2)), (2, Some(3))];
        assert_eq!(
            find_supermajority_slot(supermajority, &stakes_and_slots),
            Some(2)
        );

        let stakes_and_slots = vec![(9, Some(2)), (2, Some(3)), (9, None)];
        assert_eq!(
            find_supermajority_slot(supermajority, &stakes_and_slots),
            Some(2)
        );

        let stakes_and_slots = vec![(9, Some(2)), (2, Some(3)), (9, Some(3))];
        assert_eq!(
            find_supermajority_slot(supermajority, &stakes_and_slots),
            Some(3)
        );
    }
}

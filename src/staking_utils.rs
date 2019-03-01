use hashbrown::HashMap;
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::vote_program::VoteState;

/// Looks through vote accounts, and finds the latest slot that has achieved
/// supermajority lockout
pub fn get_supermajority_slot(bank: &Bank, epoch_height: u64) -> Option<u64> {
    // Find the amount of stake needed for supermajority
    let stakes_and_lockouts = epoch_stakes_and_lockouts(bank, epoch_height);
    let total_stake: u64 = stakes_and_lockouts.values().map(|s| s.0).sum();
    let supermajority_stake = total_stake * 2 / 3;

    // Filter out the states that don't have a max lockout
    find_supermajority_slot(supermajority_stake, stakes_and_lockouts.values())
}

/// Collect the node Pubkey and staker account balance for nodes
/// that have non-zero balance in their corresponding staking accounts
pub fn node_stakes(bank: &Bank) -> HashMap<Pubkey, u64> {
    node_stakes_extractor(bank, |stake, _| stake)
}

/// Return the checkpointed stakes that should be used to generate a leader schedule.
pub fn node_stakes_at_epoch(bank: &Bank, epoch_height: u64) -> HashMap<Pubkey, u64> {
    node_stakes_at_epoch_extractor(bank, epoch_height, |stake, _| stake)
}

/// Return the checkpointed stakes that should be used to generate a leader schedule.
/// state_extractor takes (stake, vote_state) and maps to an output.
fn node_stakes_at_epoch_extractor<F, T>(
    bank: &Bank,
    epoch_height: u64,
    state_extractor: F,
) -> HashMap<Pubkey, T>
where
    F: Fn(u64, &VoteState) -> T,
{
    let epoch_slot_height = epoch_height * bank.slots_per_epoch();
    node_stakes_at_slot_extractor(bank, epoch_slot_height, state_extractor)
}

/// Return the checkpointed stakes that should be used to generate a leader schedule.
/// state_extractor takes (stake, vote_state) and maps to an output
fn node_stakes_at_slot_extractor<F, T>(
    bank: &Bank,
    current_slot_height: u64,
    state_extractor: F,
) -> HashMap<Pubkey, T>
where
    F: Fn(u64, &VoteState) -> T,
{
    let slot_height = current_slot_height.saturating_sub(bank.stakers_slot_offset());

    let parents = bank.parents();
    let mut banks = vec![bank];
    banks.extend(parents.iter().map(|x| x.as_ref()));

    let bank = banks
        .iter()
        .find(|bank| bank.slot() <= slot_height)
        .unwrap_or_else(|| banks.last().unwrap());

    node_stakes_extractor(bank, state_extractor)
}

/// Collect the node Pubkey and staker account balance for nodes
/// that have non-zero balance in their corresponding staker accounts.
/// state_extractor takes (stake, vote_state) and maps to an output
fn node_stakes_extractor<F, T>(bank: &Bank, state_extractor: F) -> HashMap<Pubkey, T>
where
    F: Fn(u64, &VoteState) -> T,
{
    bank.vote_states(|account_id, _| bank.get_balance(&account_id) > 0)
        .iter()
        .map(|(account_id, state)| {
            (
                state.delegate_id,
                state_extractor(bank.get_balance(&account_id), &state),
            )
        })
        .collect()
}

fn epoch_stakes_and_lockouts(
    bank: &Bank,
    epoch_height: u64,
) -> HashMap<Pubkey, (u64, Option<u64>)> {
    node_stakes_at_epoch_extractor(bank, epoch_height, |stake, state| (stake, state.root_slot))
}

fn find_supermajority_slot<'a, I>(supermajority_stake: u64, stakes_and_lockouts: I) -> Option<u64>
where
    I: Iterator<Item = &'a (u64, Option<u64>)>,
{
    // Filter out the states that don't have a max lockout
    let mut stakes_and_lockouts: Vec<_> = stakes_and_lockouts
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voting_keypair::tests as voting_keypair_tests;
    use hashbrown::HashSet;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::iter::FromIterator;
    use std::sync::Arc;

    fn register_ticks(bank: &Bank, n: u64) -> (u64, u64, u64) {
        for _ in 0..n {
            bank.register_tick(&Hash::default());
        }
        (bank.tick_index(), bank.slot_index(), bank.epoch_height())
    }

    #[test]
    fn test_bank_staked_nodes_at_epoch() {
        let pubkey = Keypair::new().pubkey();
        let bootstrap_tokens = 2;
        let (genesis_block, _) = GenesisBlock::new_with_leader(2, pubkey, bootstrap_tokens);
        let bank = Bank::new(&genesis_block);
        let bank = Bank::new_from_parent(&Arc::new(bank));
        let ticks_per_offset = bank.stakers_slot_offset() * bank.ticks_per_slot();
        register_ticks(&bank, ticks_per_offset);
        assert_eq!(bank.slot_height(), bank.stakers_slot_offset());

        let mut expected = HashMap::new();
        expected.insert(pubkey, bootstrap_tokens - 1);
        let bank = Bank::new_from_parent(&Arc::new(bank));
        assert_eq!(
            node_stakes_at_slot_extractor(&bank, bank.slot_height(), |s, _| s),
            expected
        );
    }

    #[test]
    fn test_epoch_stakes_and_lockouts() {
        let validator = Keypair::new();

        let (genesis_block, mint_keypair) = GenesisBlock::new(500);
        let bank = Bank::new(&genesis_block);
        let bank_voter = Keypair::new();

        // Give the validator some stake but don't setup a staking account
        bank.transfer(1, &mint_keypair, validator.pubkey(), genesis_block.hash())
            .unwrap();

        // Validator has no token staked, so they get filtered out. Only the bootstrap leader
        // created by the genesis block will get included
        let expected: Vec<_> = epoch_stakes_and_lockouts(&bank, 0)
            .values()
            .cloned()
            .collect();
        assert_eq!(expected, vec![(1, None)]);

        voting_keypair_tests::new_vote_account_with_vote(&mint_keypair, &bank_voter, &bank, 499, 0);

        let result: HashSet<_> =
            HashSet::from_iter(epoch_stakes_and_lockouts(&bank, 0).values().cloned());
        let expected: HashSet<_> = HashSet::from_iter(vec![(1, None), (499, None)]);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_find_supermajority_slot() {
        let supermajority = 10;

        let stakes_and_slots = vec![];
        assert_eq!(
            find_supermajority_slot(supermajority, stakes_and_slots.iter()),
            None
        );

        let stakes_and_slots = vec![(5, None), (5, None)];
        assert_eq!(
            find_supermajority_slot(supermajority, stakes_and_slots.iter()),
            None
        );

        let stakes_and_slots = vec![(5, None), (5, None), (9, Some(2))];
        assert_eq!(
            find_supermajority_slot(supermajority, stakes_and_slots.iter()),
            None
        );

        let stakes_and_slots = vec![(5, None), (5, None), (9, Some(2)), (1, Some(3))];
        assert_eq!(
            find_supermajority_slot(supermajority, stakes_and_slots.iter()),
            None
        );

        let stakes_and_slots = vec![(5, None), (5, None), (9, Some(2)), (2, Some(3))];
        assert_eq!(
            find_supermajority_slot(supermajority, stakes_and_slots.iter()),
            Some(2)
        );

        let stakes_and_slots = vec![(9, Some(2)), (2, Some(3)), (9, None)];
        assert_eq!(
            find_supermajority_slot(supermajority, stakes_and_slots.iter()),
            Some(2)
        );

        let stakes_and_slots = vec![(9, Some(2)), (2, Some(3)), (9, Some(3))];
        assert_eq!(
            find_supermajority_slot(supermajority, stakes_and_slots.iter()),
            Some(3)
        );
    }
}

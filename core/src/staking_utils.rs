use hashbrown::HashMap;
use solana_runtime::bank::Bank;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_vote_api::vote_state::VoteState;
use std::borrow::Borrow;

/// Looks through vote accounts, and finds the latest slot that has achieved
/// supermajority lockout
pub fn get_supermajority_slot(bank: &Bank, epoch_height: u64) -> Option<u64> {
    // Find the amount of stake needed for supermajority
    let stakes_and_lockouts = epoch_stakes_and_lockouts(bank, epoch_height);
    let total_stake: u64 = stakes_and_lockouts.iter().map(|s| s.0).sum();
    let supermajority_stake = total_stake * 2 / 3;

    // Filter out the states that don't have a max lockout
    find_supermajority_slot(supermajority_stake, stakes_and_lockouts.iter())
}

pub fn vote_account_balances(bank: &Bank) -> HashMap<Pubkey, u64> {
    let node_staked_accounts = node_staked_accounts(bank);
    node_staked_accounts
        .map(|(id, stake, _)| (id, stake))
        .collect()
}

/// Collect the delegate account balance and vote states for delegates have non-zero balance in
/// any of their managed staking accounts
pub fn delegated_stakes(bank: &Bank) -> HashMap<Pubkey, u64> {
    let node_staked_accounts = node_staked_accounts(bank);
    let node_staked_vote_states = to_vote_state(node_staked_accounts);
    to_delegated_stakes(node_staked_vote_states)
}

/// At the specified epoch, collect the node account balance and vote states for nodes that
/// have non-zero balance in their corresponding staking accounts
pub fn vote_account_balances_at_epoch(
    bank: &Bank,
    epoch_height: u64,
) -> Option<HashMap<Pubkey, u64>> {
    let node_staked_accounts = node_staked_accounts_at_epoch(bank, epoch_height);
    node_staked_accounts.map(|epoch_state| epoch_state.map(|(id, stake, _)| (*id, stake)).collect())
}

/// At the specified epoch, collect the delegate account balance and vote states for delegates
/// that have non-zero balance in any of their managed staking accounts
pub fn delegated_stakes_at_epoch(bank: &Bank, epoch_height: u64) -> Option<HashMap<Pubkey, u64>> {
    let node_staked_accounts = node_staked_accounts_at_epoch(bank, epoch_height);
    let node_staked_vote_states = node_staked_accounts.map(to_vote_state);
    node_staked_vote_states.map(to_delegated_stakes)
}

/// Collect the node account balance and vote states for nodes have non-zero balance in
/// their corresponding staking accounts
fn node_staked_accounts(bank: &Bank) -> impl Iterator<Item = (Pubkey, u64, Account)> {
    bank.vote_accounts()
        .into_iter()
        .filter_map(|(account_id, account)| {
            filter_zero_balances(&account).map(|stake| (account_id, stake, account))
        })
}

pub fn node_staked_accounts_at_epoch(
    bank: &Bank,
    epoch_height: u64,
) -> Option<impl Iterator<Item = (&Pubkey, u64, &Account)>> {
    bank.epoch_vote_accounts(epoch_height).map(|epoch_state| {
        epoch_state
            .into_iter()
            .filter_map(|(account_id, account)| {
                filter_zero_balances(account).map(|stake| (account_id, stake, account))
            })
            .filter(|(account_id, _, account)| filter_no_delegate(account_id, account))
    })
}

fn filter_no_delegate(account_id: &Pubkey, account: &Account) -> bool {
    VoteState::deserialize(&account.data)
        .map(|vote_state| vote_state.node_id != *account_id)
        .unwrap_or(false)
}

fn filter_zero_balances(account: &Account) -> Option<u64> {
    let balance = Bank::read_balance(&account);
    if balance > 0 {
        Some(balance)
    } else {
        None
    }
}

fn to_vote_state(
    node_staked_accounts: impl Iterator<Item = (impl Borrow<Pubkey>, u64, impl Borrow<Account>)>,
) -> impl Iterator<Item = (u64, VoteState)> {
    node_staked_accounts.filter_map(|(_, stake, account)| {
        VoteState::deserialize(&account.borrow().data)
            .ok()
            .map(|vote_state| (stake, vote_state))
    })
}

fn to_delegated_stakes(
    node_staked_accounts: impl Iterator<Item = (u64, VoteState)>,
) -> HashMap<Pubkey, u64> {
    let mut map: HashMap<Pubkey, u64> = HashMap::new();
    node_staked_accounts.for_each(|(stake, state)| {
        let delegate = &state.node_id;
        map.entry(*delegate)
            .and_modify(|s| *s += stake)
            .or_insert(stake);
    });
    map
}

fn epoch_stakes_and_lockouts(bank: &Bank, epoch_height: u64) -> Vec<(u64, Option<u64>)> {
    let node_staked_accounts =
        node_staked_accounts_at_epoch(bank, epoch_height).expect("Bank state for epoch is missing");
    let node_staked_vote_states = to_vote_state(node_staked_accounts);
    node_staked_vote_states
        .map(|(stake, states)| (stake, states.root_slot))
        .collect()
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
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::iter::FromIterator;
    use std::sync::Arc;

    fn new_from_parent(parent: &Arc<Bank>, slot: u64) -> Bank {
        Bank::new_from_parent(parent, &Pubkey::default(), slot)
    }

    #[test]
    fn test_bank_staked_nodes_at_epoch() {
        let pubkey = Pubkey::new_rand();
        let bootstrap_lamports = 2;
        let (genesis_block, _) =
            GenesisBlock::new_with_leader(bootstrap_lamports, &pubkey, bootstrap_lamports);
        let bank = Bank::new(&genesis_block);

        // Epoch doesn't exist
        let mut expected = HashMap::new();
        assert_eq!(vote_account_balances_at_epoch(&bank, 10), None);

        // First epoch has the bootstrap leader
        expected.insert(genesis_block.bootstrap_leader_vote_account_id, 1);
        let expected = Some(expected);
        assert_eq!(vote_account_balances_at_epoch(&bank, 0), expected);

        // Second epoch carries same information
        let bank = new_from_parent(&Arc::new(bank), 1);
        assert_eq!(vote_account_balances_at_epoch(&bank, 0), expected);
        assert_eq!(vote_account_balances_at_epoch(&bank, 1), expected);
    }

    #[test]
    fn test_epoch_stakes_and_lockouts() {
        let validator = Keypair::new();

        let (genesis_block, mint_keypair) = GenesisBlock::new(500);

        let bank = Bank::new(&genesis_block);
        let bank_voter = Keypair::new();

        // Give the validator some stake but don't setup a staking account
        // Validator has no lamports staked, so they get filtered out. Only the bootstrap leader
        // created by the genesis block will get included
        bank.transfer(1, &mint_keypair, &validator.pubkey())
            .unwrap();

        // Make a mint vote account. Because the mint has nonzero stake, this
        // should show up in the active set
        voting_keypair_tests::new_vote_account(
            &mint_keypair,
            &bank_voter,
            &mint_keypair.pubkey(),
            &bank,
            499,
        );

        // soonest slot that could be a new epoch is 1
        let mut slot = 1;
        let mut epoch = bank.get_stakers_epoch(0);
        // find the first slot in the next stakers_epoch
        while bank.get_stakers_epoch(slot) == epoch {
            slot += 1;
        }

        epoch = bank.get_stakers_epoch(slot);

        let bank = new_from_parent(&Arc::new(bank), slot);

        let result: Vec<_> = epoch_stakes_and_lockouts(&bank, 0);
        assert_eq!(result, vec![(1, None)]);

        let result: HashSet<_> = HashSet::from_iter(epoch_stakes_and_lockouts(&bank, epoch));
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

    #[test]
    fn test_to_delegated_stakes() {
        let mut stakes = Vec::new();
        let delegate1 = Pubkey::new_rand();
        let delegate2 = Pubkey::new_rand();

        // Delegate 1 has stake of 3
        for i in 0..3 {
            stakes.push((i, VoteState::new(&Pubkey::new_rand(), &delegate1, 0)));
        }

        // Delegate 1 has stake of 5
        stakes.push((5, VoteState::new(&Pubkey::new_rand(), &delegate2, 0)));

        let result = to_delegated_stakes(stakes.into_iter());
        assert_eq!(result.len(), 2);
        assert_eq!(result[&delegate1], 3);
        assert_eq!(result[&delegate2], 5);
    }
}

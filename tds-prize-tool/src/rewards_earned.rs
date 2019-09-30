//! Calculates the winners of the "Most Rewards Earned" category in Tour de Sol by summing the
//! balances of all stake and vote accounts attributed to a particular validator.
//!
//! The top 3 validators will receive the top prizes and validators will be awarded additional
//! prizes if they place into the following buckets:
//!
//! `hi` - Top 25%
//! `md` - Top 25-50%
//! `lo` - Top 50-90%

use crate::prize::{self, Winner, Winners};
use solana_runtime::bank::Bank;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_stake_api::stake_state::StakeState;
use solana_vote_api::vote_state::VoteState;
use std::cmp::{max, min};
use std::collections::HashMap;

const HIGH_BUCKET: &str = "Top 25% Bucket";
const MID_BUCKET: &str = "Top 25-50% Bucket";
const LOW_BUCKET: &str = "Top 50-90% Bucket";

fn voter_stake_rewards(stake_accounts: HashMap<Pubkey, Account>) -> HashMap<Pubkey, u64> {
    let mut voter_stake_sum: HashMap<Pubkey, u64> = HashMap::new();
    for (_key, account) in stake_accounts {
        if let Some(StakeState::Stake(_authorized, _lockup, stake)) = StakeState::from(&account) {
            let stake_sum = voter_stake_sum
                .get(&stake.voter_pubkey)
                .cloned()
                .unwrap_or_default();
            voter_stake_sum.insert(stake.voter_pubkey, stake_sum + account.lamports);
        }
    }
    voter_stake_sum
}

fn validator_results(
    validator_reward_map: HashMap<Pubkey, u64>,
    starting_balance: u64,
) -> Vec<(Pubkey, i64)> {
    let mut validator_rewards: Vec<(Pubkey, u64)> = validator_reward_map
        .iter()
        .map(|(key, balance)| (*key, *balance))
        .collect();

    // Sort descending and calculate results
    validator_rewards.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    validator_rewards
        .into_iter()
        .map(|(key, earned)| (key, (earned as i64) - (starting_balance as i64)))
        .collect()
}

fn validator_rewards(
    mut voter_stake_rewards: HashMap<Pubkey, u64>,
    vote_accounts: HashMap<Pubkey, (u64, Account)>,
) -> HashMap<Pubkey, u64> {
    // Sum validator earned reward totals (stake rewards + commission)
    let mut validator_reward_map: HashMap<Pubkey, u64> = HashMap::new();
    for (voter_key, (_stake, account)) in vote_accounts {
        if let Some(vote_state) = VoteState::from(&account) {
            let voter_commission = account.lamports;
            let voter_stake_reward = voter_stake_rewards.remove(&voter_key).unwrap_or_default();

            let validator_id = vote_state.node_pubkey;
            if let Some(validator_reward) = validator_reward_map.get_mut(&validator_id) {
                *validator_reward += voter_commission + voter_stake_reward;
            } else {
                validator_reward_map.insert(validator_id, voter_commission + voter_stake_reward);
            }
        }
    }

    validator_reward_map
}

// Bucket validators for reward distribution
fn bucket_winners(results: &[(Pubkey, i64)]) -> Vec<(String, Vec<Winner>)> {
    let num_validators = results.len();
    let mut bucket_winners = Vec::new();

    // Tied winners should not end up in different buckets
    let handle_ties = |mut index: usize| -> usize {
        while (index + 1 < num_validators) && (results[index].1 == results[index + 1].1) {
            index += 1;
        }
        index
    };

    // Top 25% of validators
    let hi_bucket_index = handle_ties(max(1, num_validators / 4) - 1);
    let hi = &results[..=hi_bucket_index];
    bucket_winners.push((HIGH_BUCKET.to_string(), normalize_winners(hi)));

    // Top 25-50% of validators
    let md_bucket_index = handle_ties(max(1, num_validators / 2) - 1);
    let md = &results[(hi_bucket_index + 1)..=md_bucket_index];
    bucket_winners.push((MID_BUCKET.to_string(), normalize_winners(md)));

    // Top 50-90% of validators
    let lo_bucket_index = handle_ties(max(1, 9 * num_validators / 10) - 1);
    let lo = &results[(md_bucket_index + 1)..=lo_bucket_index];
    bucket_winners.push((LOW_BUCKET.to_string(), normalize_winners(lo)));

    bucket_winners
}

fn normalize_winners(winners: &[(Pubkey, i64)]) -> Vec<(Pubkey, String)> {
    winners
        .iter()
        .map(|(key, earned)| {
            (
                *key,
                format!("Earned {} lamports in stake rewards and commission", earned),
            )
        })
        .collect()
}

pub fn compute_winners(bank: &Bank, starting_balance: u64) -> Winners {
    let voter_stake_rewards = voter_stake_rewards(bank.stake_accounts());
    let validator_reward_map = validator_rewards(voter_stake_rewards, bank.vote_accounts());
    let results = validator_results(validator_reward_map, starting_balance);
    let num_validators = results.len();
    let num_winners = min(num_validators, 3);

    Winners {
        category: prize::Category::RewardsEarned,
        top_winners: normalize_winners(&results[..num_winners]),
        bucket_winners: bucket_winners(&results),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_stake_api::stake_state::{Authorized, Lockup, Stake};
    use solana_vote_api::vote_state::VoteInit;

    #[test]
    fn test_validator_results() {
        let mut rewards_map = HashMap::new();
        let top_validator = Pubkey::new_rand();
        let bottom_validator = Pubkey::new_rand();
        rewards_map.insert(top_validator, 1000);
        rewards_map.insert(bottom_validator, 10);

        let results = validator_results(rewards_map, 100);
        assert_eq!(results[0], (top_validator, 900));
        assert_eq!(results[1], (bottom_validator, -90));
    }

    #[test]
    fn test_validator_rewards() {
        let new_vote_account = |lamports: u64, validator_id: &Pubkey| -> Account {
            Account::new_data(
                lamports,
                &VoteState::new(&VoteInit {
                    node_pubkey: validator_id.clone(),
                    ..VoteInit::default()
                }),
                &Pubkey::new_rand(),
            )
            .unwrap()
        };

        let validator1 = Pubkey::new_rand();
        let validator2 = Pubkey::new_rand();

        let mut vote_accounts = HashMap::new();
        let voter1 = Pubkey::new_rand();
        vote_accounts.insert(voter1.clone(), (0, new_vote_account(100, &validator1)));
        vote_accounts.insert(Pubkey::new_rand(), (0, new_vote_account(100, &validator2)));
        vote_accounts.insert(Pubkey::new_rand(), (0, new_vote_account(100, &validator2)));

        let voter_stake_rewards = {
            let mut map = HashMap::new();
            map.insert(voter1, 1000);
            map
        };

        let expected_rewards = {
            let mut map = HashMap::new();
            map.insert(validator1, 1100);
            map.insert(validator2, 200);
            map
        };

        assert_eq!(
            expected_rewards,
            validator_rewards(voter_stake_rewards, vote_accounts)
        );
    }

    #[test]
    fn test_voter_stake_rewards() {
        let new_stake_account = |lamports: u64, voter_pubkey: &Pubkey| -> Account {
            Account::new_data(
                lamports,
                &StakeState::Stake(
                    Authorized::default(),
                    Lockup::default(),
                    Stake {
                        voter_pubkey: voter_pubkey.clone(),
                        ..Stake::default()
                    },
                ),
                &Pubkey::new_rand(),
            )
            .unwrap()
        };

        let voter_pubkey1 = Pubkey::new_rand();
        let voter_pubkey2 = Pubkey::new_rand();

        let mut stake_accounts = HashMap::new();
        stake_accounts.insert(Pubkey::new_rand(), new_stake_account(100, &voter_pubkey1));
        stake_accounts.insert(Pubkey::new_rand(), new_stake_account(100, &voter_pubkey2));
        stake_accounts.insert(Pubkey::new_rand(), new_stake_account(100, &voter_pubkey2));

        let expected = {
            let mut map = HashMap::new();
            map.insert(voter_pubkey1, 100);
            map.insert(voter_pubkey2, 200);
            map
        };

        assert_eq!(expected, voter_stake_rewards(stake_accounts));
    }

    #[test]
    fn test_bucket_winners() {
        let mut results = Vec::new();

        let expected_hi_bucket = vec![(Pubkey::new_rand(), 8_000), (Pubkey::new_rand(), 7_000)];

        let expected_md_bucket = vec![(Pubkey::new_rand(), 6_000), (Pubkey::new_rand(), 5_000)];

        let expected_lo_bucket = vec![
            (Pubkey::new_rand(), 4_000),
            (Pubkey::new_rand(), 3_000),
            (Pubkey::new_rand(), 2_000),
        ];

        results.extend(expected_hi_bucket.iter());
        results.extend(expected_md_bucket.iter());
        results.extend(expected_lo_bucket.iter());
        results.push((Pubkey::new_rand(), 1_000));

        let bucket_winners = bucket_winners(&results);

        assert_eq!(bucket_winners[0].1, normalize_winners(&expected_hi_bucket));
        assert_eq!(bucket_winners[1].1, normalize_winners(&expected_md_bucket));
        assert_eq!(bucket_winners[2].1, normalize_winners(&expected_lo_bucket));
    }

    #[test]
    fn test_bucket_winners_with_ties() {
        let mut results = Vec::new();

        // Ties should all get bucketed together
        let expected_hi_bucket = vec![
            (Pubkey::new_rand(), 8_000),
            (Pubkey::new_rand(), 7_000),
            (Pubkey::new_rand(), 7_000),
            (Pubkey::new_rand(), 7_000),
        ];

        let expected_md_bucket = vec![];

        let expected_lo_bucket = vec![
            (Pubkey::new_rand(), 4_000),
            (Pubkey::new_rand(), 3_000),
            (Pubkey::new_rand(), 2_000),
        ];

        results.extend(expected_hi_bucket.iter());
        results.extend(expected_md_bucket.iter());
        results.extend(expected_lo_bucket.iter());
        results.push((Pubkey::new_rand(), 1_000));

        let bucket_winners = bucket_winners(&results);

        assert_eq!(bucket_winners[0].1, normalize_winners(&expected_hi_bucket));
        assert_eq!(bucket_winners[1].1, normalize_winners(&expected_md_bucket));
        assert_eq!(bucket_winners[2].1, normalize_winners(&expected_lo_bucket));
    }
}

use {
    crate::bank::StakeRewards,
    solana_sdk::{epoch_rewards_hasher::EpochRewardsHasher, hash::Hash},
};

pub(crate) fn hash_rewards_into_partitions(
    stake_rewards: StakeRewards,
    parent_blockhash: &Hash,
    num_partitions: usize,
) -> Vec<StakeRewards> {
    let hasher = EpochRewardsHasher::new(num_partitions, parent_blockhash);
    let mut result = vec![vec![]; num_partitions];

    for reward in stake_rewards {
        // clone here so the hasher's state is re-used on each call to `hash_address_to_partition`.
        // This prevents us from re-hashing the seed each time.
        // The clone is explicit (as opposed to an implicit copy) so it is clear this is intended.
        let partition_index = hasher
            .clone()
            .hash_address_to_partition(&reward.stake_pubkey);
        result[partition_index].push(reward);
    }
    result
}

#[cfg(test)]
mod tests {
    use {super::*, solana_accounts_db::stake_rewards::StakeReward, std::collections::HashMap};

    #[test]
    fn test_hash_rewards_into_partitions() {
        // setup the expected number of stake rewards
        let expected_num = 12345;

        let stake_rewards = (0..expected_num)
            .map(|_| StakeReward::new_random())
            .collect::<Vec<_>>();

        let total = stake_rewards
            .iter()
            .map(|stake_reward| stake_reward.stake_reward_info.lamports)
            .sum::<i64>();

        let stake_rewards_in_bucket =
            hash_rewards_into_partitions(stake_rewards.clone(), &Hash::default(), 5);

        let stake_rewards_in_bucket_clone =
            stake_rewards_in_bucket.iter().flatten().cloned().collect();
        compare(&stake_rewards, &stake_rewards_in_bucket_clone);

        let total_after_hash_partition = stake_rewards_in_bucket
            .iter()
            .flatten()
            .map(|stake_reward| stake_reward.stake_reward_info.lamports)
            .sum::<i64>();

        let total_num_after_hash_partition: usize =
            stake_rewards_in_bucket.iter().map(|x| x.len()).sum();

        // assert total is same, so nothing is dropped or duplicated
        assert_eq!(total, total_after_hash_partition);
        assert_eq!(expected_num, total_num_after_hash_partition);
    }

    #[test]
    fn test_hash_rewards_into_partitions_empty() {
        let stake_rewards = vec![];
        let total = 0;

        let num_partitions = 5;
        let stake_rewards_in_bucket =
            hash_rewards_into_partitions(stake_rewards, &Hash::default(), num_partitions);

        let total_after_hash_partition = stake_rewards_in_bucket
            .iter()
            .flatten()
            .map(|stake_reward| stake_reward.stake_reward_info.lamports)
            .sum::<i64>();

        assert_eq!(total, total_after_hash_partition);

        assert_eq!(stake_rewards_in_bucket.len(), num_partitions);
        for bucket in stake_rewards_in_bucket.iter().take(num_partitions) {
            assert!(bucket.is_empty());
        }
    }

    fn compare(a: &StakeRewards, b: &StakeRewards) {
        let mut a = a
            .iter()
            .map(|stake_reward| (stake_reward.stake_pubkey, stake_reward.clone()))
            .collect::<HashMap<_, _>>();
        b.iter().for_each(|stake_reward| {
            let reward = a.remove(&stake_reward.stake_pubkey).unwrap();
            assert_eq!(&reward, stake_reward);
        });
        assert!(a.is_empty());
    }
}

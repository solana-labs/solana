use {
    crate::bank::StakeRewards,
    siphasher::sip::SipHasher13,
    solana_sdk::{hash::Hash, pubkey::Pubkey},
    std::hash::Hasher,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct EpochRewardHasher {
    hasher: SipHasher13,
    partitions: usize,
}

impl EpochRewardHasher {
    /// Use SipHasher13 keyed on the `seed` for calculating epoch reward partition
    pub(crate) fn new(partitions: usize, hash: &Hash) -> Self {
        let mut hasher = SipHasher13::new();
        hasher.write(hash.as_ref());
        Self { hasher, partitions }
    }

    /// Return partition index (0..partitions) by hashing `address` with the `hasher`
    pub(crate) fn hash_address_to_partition(self, address: &Pubkey) -> usize {
        let Self {
            mut hasher,
            partitions,
        } = self;
        hasher.write(address.as_ref());
        let hash64 = hasher.finish();

        hash_to_partition(hash64, partitions)
    }
}

/// Compute the partition index by modulo the address hash to number of partitions w.o bias.
/// (rand_int * DESIRED_RANGE_MAX) / (RAND_MAX + 1)
fn hash_to_partition(hash: u64, partitions: usize) -> usize {
    ((partitions as u128)
        .saturating_mul(u128::from(hash))
        .saturating_div(u128::from(u64::MAX).saturating_add(1))) as usize
}

#[allow(dead_code)]
fn hash_rewards_into_partitions(
    stake_rewards: &StakeRewards,
    parent_block_hash: &Hash,
    num_partitions: usize,
) -> Vec<StakeRewards> {
    let hasher = EpochRewardHasher::new(num_partitions, parent_block_hash);
    let mut result = vec![vec![]; num_partitions];

    for reward in stake_rewards {
        let partition_index = hasher.hash_address_to_partition(&reward.stake_pubkey);
        result[partition_index].push(reward.clone());
    }
    result
}

#[cfg(test)]
mod tests {
    use {super::*, crate::stake_rewards::StakeReward, std::collections::HashMap};

    #[test]
    fn test_hash_to_partitions() {
        let partitions = 16;
        assert_eq!(hash_to_partition(0, partitions), 0);
        assert_eq!(hash_to_partition(u64::MAX / 16, partitions), 0);
        assert_eq!(hash_to_partition(u64::MAX / 16 + 1, partitions), 1);
        assert_eq!(hash_to_partition(u64::MAX / 16 * 2, partitions), 1);
        assert_eq!(hash_to_partition(u64::MAX / 16 * 2 + 1, partitions), 1);
        assert_eq!(hash_to_partition(u64::MAX - 1, partitions), partitions - 1);
        assert_eq!(hash_to_partition(u64::MAX, partitions), partitions - 1);
    }

    /// Make sure that each time hash_address_to_partition is called, the hasher is copied.
    #[test]
    fn test_hasher_copy() {
        let hasher = EpochRewardHasher::new(10, &Hash::new_unique());

        let pk = Pubkey::new_unique();

        let b1 = hasher.hash_address_to_partition(&pk);
        let b2 = hasher.hash_address_to_partition(&pk);
        assert_eq!(b1, b2);
    }

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
            hash_rewards_into_partitions(&stake_rewards, &Hash::default(), 5);

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
            hash_rewards_into_partitions(&stake_rewards, &Hash::default(), num_partitions);

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

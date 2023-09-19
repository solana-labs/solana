use {
    crate::bank::StakeRewards,
    siphasher::sip::SipHasher13,
    solana_sdk::{hash::Hash, pubkey::Pubkey},
    std::hash::Hasher,
};

#[derive(Debug, Clone)]
pub(crate) struct EpochRewardsHasher {
    hasher: SipHasher13,
    partitions: usize,
}

impl EpochRewardsHasher {
    /// Use SipHasher13 keyed on the `seed` for calculating epoch reward partition
    pub(crate) fn new(partitions: usize, seed: &Hash) -> Self {
        let mut hasher = SipHasher13::new();
        hasher.write(seed.as_ref());
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

pub(crate) fn hash_rewards_into_partitions(
    stake_rewards: StakeRewards,
    parent_block_hash: &Hash,
    num_partitions: usize,
) -> Vec<StakeRewards> {
    let hasher = EpochRewardsHasher::new(num_partitions, parent_block_hash);
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
    use {
        super::*,
        solana_accounts_db::stake_rewards::StakeReward,
        std::{collections::HashMap, ops::RangeInclusive},
    };

    #[test]
    fn test_get_equal_partition_range() {
        // show how 2 equal partition ranges are 0..=(max/2), (max/2+1)..=max
        // the inclusive is tricky to think about
        let range = get_equal_partition_range(0, 2);
        assert_eq!(*range.start(), 0);
        assert_eq!(*range.end(), u64::MAX / 2);
        let range = get_equal_partition_range(1, 2);
        assert_eq!(*range.start(), u64::MAX / 2 + 1);
        assert_eq!(*range.end(), u64::MAX);
    }

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

    fn test_partitions(partition: usize, partitions: usize) {
        let partition = partition.min(partitions - 1);
        let range = get_equal_partition_range(partition, partitions);
        // beginning and end of this partition
        assert_eq!(hash_to_partition(*range.start(), partitions), partition);
        assert_eq!(hash_to_partition(*range.end(), partitions), partition);
        if partition < partitions - 1 {
            // first index in next partition
            assert_eq!(
                hash_to_partition(*range.end() + 1, partitions),
                partition + 1
            );
        } else {
            assert_eq!(*range.end(), u64::MAX);
        }
        if partition > 0 {
            // last index in previous partition
            assert_eq!(
                hash_to_partition(*range.start() - 1, partitions),
                partition - 1
            );
        } else {
            assert_eq!(*range.start(), 0);
        }
    }

    #[test]
    fn test_hash_to_partitions_equal_ranges() {
        for partitions in [2, 4, 8, 16, 4096] {
            assert_eq!(hash_to_partition(0, partitions), 0);
            for partition in [0, 1, 2, partitions - 1] {
                test_partitions(partition, partitions);
            }

            let range = get_equal_partition_range(0, partitions);
            for partition in 1..partitions {
                let this_range = get_equal_partition_range(partition, partitions);
                assert_eq!(
                    this_range.end() - this_range.start(),
                    range.end() - range.start()
                );
            }
        }
        // verify non-evenly divisible partitions (partitions will be different sizes by at most 1 from any other partition)
        for partitions in [3, 19, 1019, 4095] {
            for partition in [0, 1, 2, partitions - 1] {
                test_partitions(partition, partitions);
            }
            let expected_len_of_partition =
                ((u128::from(u64::MAX) + 1) / partitions as u128) as u64;
            for partition in 0..partitions {
                let this_range = get_equal_partition_range(partition, partitions);
                let len = this_range.end() - this_range.start();
                // size is same or 1 less
                assert!(
                    len == expected_len_of_partition || len + 1 == expected_len_of_partition,
                    "{}, {}, {}, {}",
                    expected_len_of_partition,
                    len,
                    partition,
                    partitions
                );
            }
        }
    }

    /// return start and end_inclusive of `partition` indexes out of from u64::MAX+1 elements in equal `partitions`
    /// These will be equal as long as (u64::MAX + 1) divides by `partitions` evenly
    fn get_equal_partition_range(partition: usize, partitions: usize) -> RangeInclusive<u64> {
        let max_inclusive = u128::from(u64::MAX);
        let max_plus_1 = max_inclusive + 1;
        let partition = partition as u128;
        let partitions = partitions as u128;
        let mut start = max_plus_1 * partition / partitions;
        if partition > 0 && start * partitions / max_plus_1 == partition - 1 {
            // partitions don't evenly divide and the start of this partition needs to be 1 greater
            start += 1;
        }

        let mut end_inclusive = start + max_plus_1 / partitions - 1;
        if partition < partitions.saturating_sub(1) {
            let next = end_inclusive + 1;
            if next * partitions / max_plus_1 == partition {
                // this partition is far enough into partitions such that the len of this partition is 1 larger than expected
                end_inclusive += 1;
            }
        } else {
            end_inclusive = max_inclusive;
        }
        RangeInclusive::new(start as u64, end_inclusive as u64)
    }

    /// Make sure that each time hash_address_to_partition is called, it uses the initial seed state and that clone correctly copies the initial hasher state.
    #[test]
    fn test_hasher_copy() {
        let seed = Hash::new_unique();
        let partitions = 10;
        let hasher = EpochRewardsHasher::new(partitions, &seed);

        let pk = Pubkey::new_unique();

        let b1 = hasher.clone().hash_address_to_partition(&pk);
        let b2 = hasher.hash_address_to_partition(&pk);
        assert_eq!(b1, b2);

        // make sure b1 includes the seed's hash
        let mut hasher = SipHasher13::new();
        hasher.write(seed.as_ref());
        hasher.write(pk.as_ref());
        let partition = hash_to_partition(hasher.finish(), partitions);
        assert_eq!(partition, b1);
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

use {
    siphasher::sip::SipHasher13,
    solana_sdk::{hash::Hash, pubkey::Pubkey},
    std::hash::Hasher,
};

#[derive(Debug, Clone)]
pub struct EpochRewardsHasher {
    hasher: SipHasher13,
    partitions: usize,
}

impl EpochRewardsHasher {
    /// Use SipHasher13 keyed on the `seed` for calculating epoch reward partition
    pub fn new(partitions: usize, seed: &Hash) -> Self {
        let mut hasher = SipHasher13::new();
        hasher.write(seed.as_ref());
        Self { hasher, partitions }
    }

    /// Return partition index (0..partitions) by hashing `address` with the `hasher`
    pub fn hash_address_to_partition(self, address: &Pubkey) -> usize {
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
// Clippy objects to `u128::from(u64::MAX).saturating_add(1)`, even though it
// can never overflow
#[allow(clippy::arithmetic_side_effects)]
fn hash_to_partition(hash: u64, partitions: usize) -> usize {
    ((partitions as u128)
        .saturating_mul(u128::from(hash))
        .saturating_div(u128::from(u64::MAX).saturating_add(1))) as usize
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects)]
    use {super::*, std::ops::RangeInclusive};

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
}

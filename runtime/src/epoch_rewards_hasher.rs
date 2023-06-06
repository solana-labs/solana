use {
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
    pub fn new(partitions: usize, hash: &Hash) -> Self {
        let mut hasher = SipHasher13::new();
        hasher.write(hash.as_ref());
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

        // Compute the partition index by modulo the address hash to number of partitions w.o bias
        ((partitions as u128)
            .saturating_mul(u128::from(hash64))
            .saturating_div(u128::from(u64::MAX).saturating_add(1))) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Make sure that each time hash_address_to_partition is called, the hasher is copied.
    #[test]
    fn test_hasher_copy() {
        let hasher = EpochRewardHasher::new(10, &Hash::new_unique());

        let pk = Pubkey::new_unique();

        let b1 = hasher.hash_address_to_partition(&pk);
        let b2 = hasher.hash_address_to_partition(&pk);
        assert_eq!(b1, b2);
    }
}

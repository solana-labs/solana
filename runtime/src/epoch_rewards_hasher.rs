use {
    siphasher::sip::SipHasher13,
    solana_sdk::{hash::Hash, pubkey::Pubkey},
    std::hash::Hasher,
};

#[derive(Debug, Clone)]
pub(crate) struct EpochRewardHasher{
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
        let Self { mut hasher, partitions } = self;
        hasher.write(address.as_ref());
        let hash64 = hasher.finish();
        ((partitions as u128)
            .saturating_mul(u128::from(hash64))
            .saturating_div(u128::from(u64::MAX).saturating_add(1))) as usize
    }
}

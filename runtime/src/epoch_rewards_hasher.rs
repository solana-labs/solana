use {
    siphasher::sip::SipHasher13, solana_sdk::hash::Hash, solana_sdk::pubkey::Pubkey,
    std::hash::Hasher,
};

/// Use SipHasher13 keyed on the `seed` for calculating epoch reward partition
pub(crate) fn create_epoch_reward_hasher(hash: &Hash) -> SipHasher13 {
    let mut hasher = SipHasher13::new();
    hasher.write(hash.as_ref());
    hasher
}

/// Return partition index (0..partitions) by hashing `address` with the `hasher`
pub(crate) fn address_to_partition<H: Hasher + Copy>(
    mut hasher: H,
    partitions: usize,
    address: &Pubkey,
) -> usize {
    hasher.write(address.as_ref());
    let hash64 = hasher.finish();
    ((partitions as u128)
        .saturating_mul(hash64 as u128)
        .saturating_div((u64::MAX as u128).saturating_add(1))) as usize
}

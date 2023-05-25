use {siphasher::sip::SipHasher13, solana_sdk::pubkey::Pubkey, std::hash::Hasher};

/// Use SipHasher13 keyed on the `seed` for calculating epoch reward partition
pub(crate) fn create_epoch_reward_hasher(seed: u64) -> SipHasher13 {
    SipHasher13::new_with_keys(seed, seed)
}

/// Return partition index (0..partitions) by hashing `seed` and `address`
pub(crate) fn address_to_partition<H: Hasher + Clone>(
    mut hasher: H,
    partitions: usize,
    address: &Pubkey,
) -> usize {
    hasher.write(address.as_ref());
    let hash64 = hasher.finish();
    ((partitions as u128) * (hash64 as u128) / ((u64::MAX as u128) + 1)) as usize
}

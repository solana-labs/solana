use {solana_sdk::pubkey::Pubkey, std::hash::Hasher};

#[derive(Clone)]
struct Blake3Hasher(blake3::Hasher);

impl Hasher for Blake3Hasher {
    fn finish(&self) -> u64 {
        let hash = self.0.finalize();
        u64::from_le_bytes(hash.as_bytes()[..8].try_into().unwrap())
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

impl Blake3Hasher {
    fn new_with_seed(seed: u64) -> Self {
        let seed_bytes = seed.to_le_bytes();
        let mut key = [0u8; 32];
        for chunk in key.chunks_mut(8) {
            chunk.copy_from_slice(&seed_bytes);
        }
        Self(blake3::Hasher::new_keyed(&key))
    }
}

/// return partition index (0..partitions) by hashing `seed` and `address`
pub(crate) fn address_to_partition(partitions: usize, seed: u64, address: &Pubkey) -> usize {
    let mut hasher = Blake3Hasher::new_with_seed(seed);
    hasher.write(address.as_ref());
    let hash = hasher.finish();
    ((partitions as u128) * (hash as u128) / ((u64::MAX as u128) + 1)) as usize
}

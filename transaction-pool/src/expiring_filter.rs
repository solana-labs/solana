use {
    ahash::AHasher,
    rand::{thread_rng, Rng},
    solana_sdk::{hash::Hash, pubkey::Pubkey},
    std::hash::Hasher,
};

/// expiring filter
pub struct ExpiringFilter {
    buckets: Vec<u64>,
    pub seed: (u128, u128),
    age: u64,
}

impl ExpiringFilter {
    pub fn new() -> Self {
        Self {
            seed: thread_rng().gen(),
            buckets: vec![(0, 0); u16::MAX.into()],
            age: 2_000,
        }
    }

    pub fn reset(&mut self) {
        self.seed = thread_rng().gen();
        self.buckets = vec![(0, 0); u16::MAX.into()];
    }

    pub fn set(&mut self, val: &[u8], now_ms: u64) {
        let mut hasher = AHasher::new_with_keys(self.seed.0, self.seed.1);
        hasher.write(val);
        self.set_key_price(hasher.finish(), now_ms)
    }

    pub fn set_key(&mut self, key: u64, now_ms: u64) {
        let pos = key % u64::from(u16::MAX);
        self.buckets[usize::try_from(pos).unwrap()] = now_ms;
    }

    pub fn check(&self, val: &[u8], now_ms: u64) -> bool {
        let mut hasher = AHasher::new_with_keys(self.seed.0, self.seed.1);
        hasher.write(val);
        let pos = hasher.finish() % u64::from(u16::MAX);
        let item = self.buckets[usize::try_from(pos).unwrap()];
        now_ms > item.1.saturating_add(self.age)
    }
}

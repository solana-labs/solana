use {
    ahash::AHasher,
    rand::{thread_rng, Rng},
    solana_sdk::hash::Hash,
    solana_sdk::pubkey::Pubkey,
    std::hash::Hasher,
};

/// expiring price filter
pub struct FeeFilter {
    buckets: Vec<(u64, u64)>,
    pub seed: (u128, u128),
    age: u64,
}

impl FeeFilter {
    pub fn new() -> Self {
        Self {
            seed: thread_rng().gen(),
            buckets: vec![(0,0); u16::MAX.into()],
            age: 2_000,
        }
    }

    pub fn reset(&mut self) {
        self.seed = thread_rng().gen();
        self.buckets = vec![(0,0); u16::MAX.into()];
    }

    pub fn set_price(&mut self, addr: &Pubkey, lamports_per_cu: u64, now_ms: u64) {
        let mut hasher = AHasher::new_with_keys(self.seed.0, self.seed.1);
        hasher.write(addr.as_ref());
        self.set_key_price(hasher.finish(), lamports_per_cu, now_ms)
    }

    pub fn set_key_price(&mut self, key: u64, lamports_per_cu: u64, now_ms: u64) {
        let pos = key % u64::from(u16::MAX);
        self.buckets[usize::try_from(pos).unwrap()] = (lamports_per_cu, now_ms);
    }

    pub fn check_price(&self, addr: &Pubkey, lamports_per_cu: u64, now_ms: u64) -> bool {
        let mut hasher = AHasher::new_with_keys(self.seed.0, self.seed.1);
        hasher.write(addr.as_ref());
        let pos = hasher.finish() % u64::from(u16::MAX);
        let item = self.buckets[usize::try_from(pos).unwrap()];
        now_ms > item.1.saturating_add(self.age) || item.0 < lamports_per_cu
    }
}

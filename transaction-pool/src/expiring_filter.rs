use {
    ahash::AHasher,
    core::sync::atomic::{AtomicU64, Ordering},
    rand::{thread_rng, Rng},
    std::hash::Hasher,
};

/// expiring filter
pub struct ExpiringFilter {
    buckets: Vec<AtomicU64>,
    seed: [AtomicU64; 4],
    age: u64,
}

impl ExpiringFilter {
    pub fn new() -> Self {
        Self {
            seed: [
                AtomicU64::new(thread_rng().gen()),
                AtomicU64::new(thread_rng().gen()),
                AtomicU64::new(thread_rng().gen()),
                AtomicU64::new(thread_rng().gen()),
            ],
            buckets: (0..u16::MAX)
                .into_iter()
                .map(|_| AtomicU64::new(0))
                .collect(),
            age: 2_000,
        }
    }

    pub fn reset(&self) {
        //this is an inconsient reset
        //worst case is that inconsisent entries expire in Self::age ms
        self.seed[0].store(thread_rng().gen(), Ordering::Relaxed);
        self.seed[1].store(thread_rng().gen(), Ordering::Relaxed);
        self.seed[2].store(thread_rng().gen(), Ordering::Relaxed);
        self.seed[3].store(thread_rng().gen(), Ordering::Relaxed);
        for v in &self.buckets {
            v.store(0, Ordering::Relaxed);
        }
    }

    fn hasher(&self) -> AHasher {
        let seed0 = u128::from(self.seed[0].load(Ordering::Relaxed))
            << 64 + u128::from(self.seed[1].load(Ordering::Relaxed));
        let seed1 = u128::from(self.seed[2].load(Ordering::Relaxed))
            << 64 + u128::from(self.seed[3].load(Ordering::Relaxed));
        AHasher::new_with_keys(seed0, seed1)
    }

    pub fn set(&self, val: &[u8], now_ms: u64) {
        let mut hasher = self.hasher();
        hasher.write(val);
        self.set_key(hasher.finish(), now_ms)
    }

    pub fn set_key(&self, key: u64, now_ms: u64) {
        let pos = key % u64::from(u16::MAX);
        self.buckets[usize::try_from(pos).unwrap()].store(now_ms, Ordering::Relaxed);
    }

    pub fn check(&self, val: &[u8], now_ms: u64) -> bool {
        let mut hasher = self.hasher();
        hasher.write(val);
        let pos = hasher.finish() % u64::from(u16::MAX);
        let time = self.buckets[usize::try_from(pos).unwrap()].load(Ordering::Relaxed);
        now_ms > time.saturating_add(self.age)
    }
}

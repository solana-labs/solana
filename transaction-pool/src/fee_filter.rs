use {
    ahash::AHasher,
    core::sync::atomic::{AtomicU64, Ordering},
    rand::{thread_rng, Rng},
    solana_sdk::{pubkey::Pubkey},
    std::hash::Hasher,
};

/// expiring price filter
pub struct FeeFilter {
    buckets: Vec<(AtomicU64, AtomicU64)>,
    seed: [AtomicU64; 4],
    global_price: AtomicU64,
    global_now_ms: AtomicU64,
    age: u64,
}

impl FeeFilter {
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
                .map(|_| (AtomicU64::new(0), AtomicU64::new(0)))
                .collect(),
            age: 2_000,
            global_price: AtomicU64::new(0),
            global_now_ms: AtomicU64::new(0),
        }
    }

    pub fn reset(&mut self) {
        //this is an inconsient reset
        //worst case is that inconsisent entries expire in Self::age ms
        self.seed[0].store(thread_rng().gen(), Ordering::Relaxed);
        self.seed[1].store(thread_rng().gen(), Ordering::Relaxed);
        self.seed[2].store(thread_rng().gen(), Ordering::Relaxed);
        self.seed[3].store(thread_rng().gen(), Ordering::Relaxed);
        for v in &self.buckets {
            //update the time, which will expire the price
            v.1.store(0, Ordering::Relaxed);
        }
    }

    pub fn hasher(&self) -> AHasher {
        let seed0 = u128::from(self.seed[0].load(Ordering::Relaxed))
            << 64 + u128::from(self.seed[1].load(Ordering::Relaxed));
        let seed1 = u128::from(self.seed[2].load(Ordering::Relaxed))
            << 64 + u128::from(self.seed[3].load(Ordering::Relaxed));
        AHasher::new_with_keys(seed0, seed1)
    }

    pub fn set_price(&mut self, addr: &Pubkey, lamports_per_cu: u64, now_ms: u64) {
        let mut hasher = self.hasher();
        hasher.write(addr.as_ref());
        self.set_key_price(hasher.finish(), lamports_per_cu, now_ms)
    }

    pub fn set_key_price(&mut self, key: u64, lamports_per_cu: u64, now_ms: u64) {
        let pos = key % u64::from(u16::MAX);
        self.buckets[usize::try_from(pos).unwrap()]
            .0
            .store(lamports_per_cu, Ordering::Relaxed);
        self.buckets[usize::try_from(pos).unwrap()]
            .1
            .store(now_ms, Ordering::Relaxed);
    }

    pub fn set_global_price(&mut self, lamports_per_cu: u64, now_ms: u64) {
        self.global_price.store(lamports_per_cu, Ordering::Relaxed);
        self.global_now_ms.store(now_ms, Ordering::Relaxed);
    }

    pub fn check_price(&self, addr: &Pubkey, lamports_per_cu: u64, now_ms: u64) -> bool {
        let global_now_ms = self.global_now_ms.load(Ordering::Relaxed);
        let global_price = self.global_price.load(Ordering::Relaxed);
        if !(now_ms > global_now_ms.saturating_add(self.age) || lamports_per_cu < global_price) {
            return false;
        }
        let mut hasher = self.hasher();
        hasher.write(addr.as_ref());
        let pos = hasher.finish() % u64::from(u16::MAX);
        let price = self.buckets[usize::try_from(pos).unwrap()]
            .0
            .load(Ordering::Relaxed);
        let time = self.buckets[usize::try_from(pos).unwrap()]
            .1
            .load(Ordering::Relaxed);
        now_ms > time.saturating_add(self.age) || price < lamports_per_cu
    }
}

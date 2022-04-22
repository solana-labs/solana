use {
    ahash::AHasher,
    rand::{thread_rng, Rng},
    solana_sdk::pubkey::Pubkey,
    std::hash::Hasher,
};

pub struct FeePayerFilter {
    feepayers: Vec<u32>,
    count: u64,
    seed: (u128, u128),
}

impl FeePayerFilter {
    pub fn new() -> Self {
        Self {
            seed: thread_rng().gen(),
            feepayers: vec![0; u16::MAX.into()],
            count: 0,
        }
    }

    pub fn reset(&mut self) {
        self.seed = thread_rng().gen();
        self.feepayers = vec![0; u16::MAX.into()];
    }

    //accumilate invalid fee payers
    pub fn invalid(&mut self, addr: &Pubkey) {
        let mut hasher = AHasher::new_with_keys(self.seed.0, self.seed.1);
        hasher.write(addr.as_ref());
        let pos = hasher.finish() % u64::from(u16::MAX);
        self.feepayers[usize::try_from(pos).unwrap()] =
            self.feepayers[usize::try_from(pos).unwrap()].saturating_add(1);
        self.count = self.count.saturating_add(1);
    }

    //drop those that are above the expected mean
    pub fn is_invalid(&self, addr: &Pubkey) -> bool {
        let mut hasher = AHasher::new_with_keys(self.seed.0, self.seed.1);
        hasher.write(addr.as_ref());
        let pos = hasher.finish() % u64::from(u16::MAX);
        let expected =
            u64::from(self.feepayers[usize::try_from(pos).unwrap()]) * u64::from(u16::MAX);
        expected > self.count
    }
}

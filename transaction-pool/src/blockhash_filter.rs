use {
    ahash::AHasher,
    rand::{thread_rng, Rng},
    solana_sdk::hash::Hash,
    std::hash::Hasher,
    solana_perf::{
        packet::PacketBatch,
    },
};

pub struct BlockhashFilter {
    blockhashes: Vec<bool>,
    seed: (u128, u128),
}

impl BlockhashFilter {
    pub fn new() -> Self {
        Self {
            seed: thread_rng().gen(),
            blockhashes: vec![false; u16::MAX.into()],
        }
    }

    pub fn reset(&mut self) {
        self.seed = thread_rng().gen();
        self.blockhashes = vec![false; u16::MAX.into()];
    }

    pub fn valid(&mut self, hash: &Hash) {
        let mut hasher = AHasher::new_with_keys(self.seed.0, self.seed.1);
        hasher.write(hash.as_ref());
        let pos = hasher.finish() % u64::from(u16::MAX);
        self.blockhashes[usize::try_from(pos).unwrap()] = true;
    }
    pub fn invalid(&mut self, hash: &Hash) {
        let mut hasher = AHasher::new_with_keys(self.seed.0, self.seed.1);
        hasher.write(hash.as_ref());
        let pos = hasher.finish() % u64::from(u16::MAX);
        self.blockhashes[usize::try_from(pos).unwrap()] = false;
    }
    pub fn check(&self, hash: &Hash) -> bool {
        let mut hasher = AHasher::new_with_keys(self.seed.0, self.seed.1);
        hasher.write(hash.as_ref());
        let pos = hasher.finish() % u64::from(u16::MAX);
        self.blockhashes[usize::try_from(pos).unwrap()]
    }
}

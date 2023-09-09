#![feature(test)]

extern crate test;

use {
    rand::{Rng, SeedableRng},
    rand_chacha::ChaChaRng,
    solana_gossip::weighted_shuffle::WeightedShuffle,
    std::iter::repeat_with,
    test::Bencher,
};

fn make_weights<R: Rng>(rng: &mut R) -> Vec<u64> {
    repeat_with(|| rng.gen_range(1..100)).take(1000).collect()
}

#[bench]
fn bench_weighted_shuffle(bencher: &mut Bencher) {
    let mut seed = [0u8; 32];
    let mut rng = rand::thread_rng();
    let weights = make_weights(&mut rng);
    bencher.iter(|| {
        rng.fill(&mut seed[..]);
        WeightedShuffle::new("", &weights)
            .shuffle(&mut ChaChaRng::from_seed(seed))
            .collect::<Vec<_>>()
    });
}

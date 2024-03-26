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
    repeat_with(|| rng.gen_range(1..100)).take(4000).collect()
}

#[bench]
fn bench_weighted_shuffle_new(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();
    bencher.iter(|| {
        let weights = make_weights(&mut rng);
        std::hint::black_box(WeightedShuffle::new("", &weights));
    });
}

#[bench]
fn bench_weighted_shuffle_shuffle(bencher: &mut Bencher) {
    let mut seed = [0u8; 32];
    let mut rng = rand::thread_rng();
    let weights = make_weights(&mut rng);
    let weighted_shuffle = WeightedShuffle::new("", &weights);
    bencher.iter(|| {
        rng.fill(&mut seed[..]);
        let mut rng = ChaChaRng::from_seed(seed);
        let shuffle = weighted_shuffle.clone().shuffle(&mut rng);
        std::hint::black_box(shuffle.collect::<Vec<_>>());
    });
}

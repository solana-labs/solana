#![feature(test)]

extern crate test;

use {
    rand::{Rng, SeedableRng},
    rand_chacha::ChaChaRng,
    solana_gossip::weighted_shuffle::{
        weighted_best,
        weighted_shuffle,
        WeightedShuffle,
    },
    std::iter::repeat_with,
    test::Bencher,
};

fn make_weights<R: Rng>(rng: &mut R) -> Vec<u64> {
    repeat_with(|| rng.gen_range(1, 100)).take(1000).collect()
}

fn make_weights_and_indexes<R: Rng>(rng: &mut R) -> Vec<(u64, usize)> {
    (0..1_000).map(|i| (rng.gen_range(1, 100), i)).collect()
}

#[bench]
fn bench_weighted_shuffle_old(bencher: &mut Bencher) {
    let mut seed = [0u8; 32];
    let mut rng = rand::thread_rng();
    let weights = make_weights(&mut rng);
    bencher.iter(|| {
        rng.fill(&mut seed[..]);
        weighted_shuffle::<u64, &u64, std::slice::Iter<'_, u64>>(weights.iter(), seed);
    });
}

#[bench]
fn bench_weighted_shuffle_new(bencher: &mut Bencher) {
    let mut seed = [0u8; 32];
    let mut rng = rand::thread_rng();
    let weights = make_weights(&mut rng);
    bencher.iter(|| {
        rng.fill(&mut seed[..]);
        WeightedShuffle::new(&mut ChaChaRng::from_seed(seed), &weights)
            .unwrap()
            .collect::<Vec<_>>()
    });
}

#[bench]
fn bench_weighted_best(bencher: &mut Bencher) {
    let mut seed = [0u8; 32];
    let mut rng = rand::thread_rng();
    let wni = make_weights_and_indexes(&mut rng);
    bencher.iter(|| {
        rng.fill(&mut seed[..]);
        weighted_best(&wni, seed);
    });
}

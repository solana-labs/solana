#![feature(test)]

extern crate test;

use rand::{thread_rng, Rng};
use solana_core::crds_gossip_pull::CrdsFilter;
use solana_sdk::hash::{Hash, HASH_BYTES};
use test::Bencher;

#[bench]
fn bench_hash_as_u64(bencher: &mut Bencher) {
    let mut rng = thread_rng();
    let hashes: Vec<_> = (0..1000)
        .map(|_| {
            let mut buf = [0u8; HASH_BYTES];
            rng.fill(&mut buf);
            Hash::new(&buf)
        })
        .collect();
    bencher.iter(|| {
        hashes
            .iter()
            .map(CrdsFilter::hash_as_u64)
            .collect::<Vec<_>>()
    });
}

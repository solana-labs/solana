#![feature(test)]

extern crate test;

use solana_core::shred::{Shredder, RECOMMENDED_FEC_RATE};
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::sync::Arc;
use test::Bencher;

#[bench]
fn bench_shredder(bencher: &mut Bencher) {
    let kp = Arc::new(Keypair::new());
    // 1Mb
    let data = vec![0u8; 1000 * 1000];
    bencher.iter(|| {
        let mut shredder = Shredder::new(1, 0, RECOMMENDED_FEC_RATE, &kp, 0).unwrap();
        bincode::serialize_into(&mut shredder, &data).unwrap();
    })
}

#[bench]
fn bench_deshredder(bencher: &mut Bencher) {
    let kp = Arc::new(Keypair::new());
    // 10MB
    let data = vec![0u8; 10000 * 1000];
    let mut shredded = Shredder::new(1, 0, 0.0, &kp, 0).unwrap();
    let _ = bincode::serialize_into(&mut shredded, &data);
    shredded.finalize_data();
    let (_, shreds): (Vec<_>, Vec<_>) = shredded.shred_tuples.into_iter().unzip();
    bencher.iter(|| {
        let raw = &mut Shredder::deshred(&shreds).unwrap();
        assert_ne!(raw.len(), 0);
    })
}

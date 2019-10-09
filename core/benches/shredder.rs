#![feature(test)]

extern crate test;

use solana_core::entry::create_ticks;
use solana_core::shred::{
    max_ticks_per_n_shreds, Shredder, RECOMMENDED_FEC_RATE, SIZE_OF_DATA_SHRED_HEADER,
};
use solana_sdk::hash::Hash;
use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::sync::Arc;
use test::Bencher;

#[bench]
fn bench_shredder(bencher: &mut Bencher) {
    let kp = Arc::new(Keypair::new());
    let shred_size = PACKET_DATA_SIZE - *SIZE_OF_DATA_SHRED_HEADER;
    let num_shreds = ((1000 * 1000) + (shred_size - 1)) / shred_size;
    // ~1Mb
    let num_ticks = max_ticks_per_n_shreds(1) * num_shreds as u64;
    let entries = create_ticks(num_ticks, Hash::default());
    bencher.iter(|| {
        let shredder = Shredder::new(1, 0, RECOMMENDED_FEC_RATE, kp.clone()).unwrap();
        shredder.entries_to_shreds(&entries, true, 0);
    })
}

#[bench]
fn bench_deshredder(bencher: &mut Bencher) {
    let kp = Arc::new(Keypair::new());
    let shred_size = PACKET_DATA_SIZE - *SIZE_OF_DATA_SHRED_HEADER;
    // ~10Mb
    let num_shreds = ((10000 * 1000) + (shred_size - 1)) / shred_size;
    let num_ticks = max_ticks_per_n_shreds(1) * num_shreds as u64;
    let entries = create_ticks(num_ticks, Hash::default());
    let shredder = Shredder::new(1, 0, RECOMMENDED_FEC_RATE, kp).unwrap();
    let data_shreds = shredder.entries_to_shreds(&entries, true, 0).0;
    bencher.iter(|| {
        let raw = &mut Shredder::deshred(&data_shreds).unwrap();
        assert_ne!(raw.len(), 0);
    })
}

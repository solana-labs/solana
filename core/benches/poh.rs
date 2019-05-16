// This bench attempts to justify the value of `solana::poh_service::NUM_HASHES_PER_BATCH`

#![feature(test)]
extern crate test;

use solana::poh::Poh;
use solana::poh_service::NUM_HASHES_PER_BATCH;
use solana_sdk::hash::Hash;
use std::sync::{Arc, Mutex};
use test::Bencher;

const NUM_HASHES: u64 = 30_000; // Should require ~10ms on a 2017 MacBook Pro

#[bench]
// No locking.
// Fastest.
fn bench_poh_hash(bencher: &mut Bencher) {
    let mut poh = Poh::new(Hash::default(), None);
    bencher.iter(|| {
        poh.hash(NUM_HASHES);
    })
}

#[bench]
// Acquire lock on each iteration.
// Slowest.
fn bench_arc_mutex_poh_hash(bencher: &mut Bencher) {
    let poh = Arc::new(Mutex::new(Poh::new(Hash::default(), None)));
    bencher.iter(|| {
        for _ in 0..NUM_HASHES {
            poh.lock().unwrap().hash(1);
        }
    })
}

#[bench]
// Acquire lock every NUM_HASHES_PER_BATCH iterations.
// Speed will be close to bench_poh_hash() if NUM_HASHES_PER_BATCH is set well.
fn bench_arc_mutex_poh_batched_hash(bencher: &mut Bencher) {
    let poh = Arc::new(Mutex::new(Poh::new(Hash::default(), None)));
    bencher.iter(|| {
        // NOTE: This block should resemble `PohService::tick_producer()` or
        //       the `NUM_HASHES_PER_BATCH` magic number may no longer be optimal
        let mut num_hashes = NUM_HASHES;
        while num_hashes != 0 {
            let num_hashes_batch = std::cmp::min(num_hashes, NUM_HASHES_PER_BATCH);
            poh.lock().unwrap().hash(num_hashes_batch);
            num_hashes -= num_hashes_batch;
        }
    })
}

#[bench]
// Worst case transaction recording delay due to batch hashing
fn bench_poh_lock_time_per_batch(bencher: &mut Bencher) {
    let mut poh = Poh::new(Hash::default(), None);
    bencher.iter(|| {
        poh.hash(NUM_HASHES_PER_BATCH);
    })
}

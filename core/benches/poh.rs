// This bench attempts to justify the value of `solana_core::poh_service::NUM_HASHES_PER_BATCH`

#![feature(test)]
extern crate test;

use core_affinity;
use solana_core::poh::Poh;
use solana_core::poh_service::NUM_HASHES_PER_BATCH;
use solana_sdk::hash::hash;
use solana_sdk::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::Builder;
use std::time::Instant;
use test::Bencher;

const NUM_HASHES: u64 = 30_000; // Should require ~10ms on a 2017 MacBook Pro

#[bench]
// No locking.  Fastest.
fn bench_poh_hash(bencher: &mut Bencher) {
    let mut poh = Poh::new(Hash::default(), None);
    bencher.iter(|| {
        poh.hash(NUM_HASHES);
    })
}

#[bench]
// Lock on each iteration.  Slowest.
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
// Speed should be close to bench_poh_hash() if NUM_HASHES_PER_BATCH is set well.
fn bench_arc_mutex_poh_batched_hash(bencher: &mut Bencher) {
    let poh = Arc::new(Mutex::new(Poh::new(Hash::default(), Some(NUM_HASHES))));
    //let exit = Arc::new(AtomicBool::new(false));
    let exit = Arc::new(AtomicBool::new(true));

    bencher.iter(|| {
        // NOTE: This block attempts to look as close as possible to `PohService::tick_producer()`
        loop {
            if poh.lock().unwrap().hash(NUM_HASHES_PER_BATCH) {
                poh.lock().unwrap().tick().unwrap();
                if exit.load(Ordering::Relaxed) {
                    break;
                }
            }
        }
    })
}

#[bench]
// Worst case transaction record delay due to batch hashing at NUM_HASHES_PER_BATCH
fn bench_poh_lock_time_per_batch(bencher: &mut Bencher) {
    let mut poh = Poh::new(Hash::default(), None);
    bencher.iter(|| {
        poh.hash(NUM_HASHES_PER_BATCH);
    })
}

#[bench]
#[ignore]
fn bench_multi_core_poh(bencher: &mut Bencher) {
    let num_threads = 1;
    let threads: Vec<_> = (0..num_threads)
        .map(|i| {
            Builder::new()
                .name("solana-poh".to_string())
                .spawn(move || {
                    if i == 0 {
                        if let Some(cores) = core_affinity::get_core_ids() {
                            core_affinity::set_for_current(cores[0]);
                        }
                    }
                    let mut v = Hash::default();
                    let start = Instant::now();
                    for _ in 0..1_000_000 {
                        v = hash(&v.as_ref());
                    }
                    let elapsed = start.elapsed().as_millis();
                    println!("thread {}, elapsed: {}", i, elapsed);
                })
                .unwrap()
        })
        .collect();

    for t in threads {
        let _ = t.join();
    }
}

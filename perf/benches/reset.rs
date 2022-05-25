#![feature(test)]

extern crate test;

use {
    std::sync::atomic::{AtomicU64, Ordering},
    test::Bencher,
};

const N: usize = 1_000_000;

// test bench_reset1 ... bench:     436,240 ns/iter (+/- 176,714)
// test bench_reset2 ... bench:     274,007 ns/iter (+/- 129,552)

#[bench]
fn bench_reset1(bencher: &mut Bencher) {
    solana_logger::setup();

    let mut v = Vec::with_capacity(N);
    v.resize_with(N, AtomicU64::default);

    bencher.iter(|| {
        test::black_box({
            for i in &v {
                i.store(0, Ordering::Relaxed);
            }
            0
        });
    });
}

#[bench]
fn bench_reset2(bencher: &mut Bencher) {
    solana_logger::setup();

    let mut v = Vec::with_capacity(N);
    v.resize_with(N, AtomicU64::default);

    bencher.iter(|| {
        test::black_box({
            v.clear();
            v.resize_with(N, AtomicU64::default);
            0
        });
    });
}

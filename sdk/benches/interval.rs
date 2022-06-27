#![feature(test)]

extern crate test;
use {
    solana_sdk::timing::{AtomicInterval, NonAtomicInterval},
    test::Bencher,
};

#[bench]
fn bench_atomic_interval(b: &mut Bencher) {
    let interval = AtomicInterval::default();
    b.iter(|| {
        let mut _c = 0;
        for _ in 0..1_00 {
            if interval.should_update(1) {
                _c += 1;
            }
        }
    });
}

#[bench]
fn bench_non_atomic_interval(b: &mut Bencher) {
    let mut interval = NonAtomicInterval::default();
    b.iter(|| {
        let mut _c = 0;
        for _ in 0..1_00 {
            if interval.should_update(1) {
                _c += 1;
            }
        }
    });
}

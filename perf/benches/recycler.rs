#![feature(test)]

extern crate test;

use {
    solana_perf::{packet::BatchRecycler, recycler::Recycler},
    solana_sdk::packet::Packet,
    test::Bencher,
};

#[bench]
fn bench_recycler(bencher: &mut Bencher) {
    solana_logger::setup();

    let recycler: BatchRecycler<Packet> = Recycler::default();

    for _ in 0..1000 {
        let _packet = recycler.allocate("");
    }

    bencher.iter(move || {
        let _packet = recycler.allocate("");
    });
}

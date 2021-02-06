#![feature(test)]

extern crate test;

use solana_perf::recycler::Recycler;
use test::Bencher;

#[bench]
fn bench_recycler(bencher: &mut Bencher) {
    solana_logger::setup();

    let recycler = Recycler::default();

    for _ in 0..1000 {
        let mut p = solana_perf::packet::Packets::with_capacity(128);
        p.packets.resize(128, solana_sdk::packet::Packet::default());
        recycler.recycle_for_test(p.packets);
    }

    bencher.iter(move || {
        recycler.recycle_for_test(recycler.allocate("me"));
    });
}

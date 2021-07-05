#![feature(test)]

extern crate test;

use solana_perf::{packet::PacketsRecycler, recycler::Recycler};

use test::Bencher;

#[bench]
fn bench_recycler(bencher: &mut Bencher) {
    solana_logger::setup();

    let recycler: PacketsRecycler = Recycler::new_without_limit("me");

    for _ in 0..1000 {
        recycler.recycle_for_test(recycler.allocate().expect("There is no limit"));
    }

    bencher.iter(move || {
        recycler.recycle_for_test(recycler.allocate().expect("There is no limit"));
    });
}

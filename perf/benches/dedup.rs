#![feature(test)]

extern crate test;

use {
    solana_perf::{packet::to_packet_batches, sigverify, test_tx::test_tx},
    test::Bencher,
};

const NUM: usize = 10_000;
#[bench]
fn bench_dedup_same(bencher: &mut Bencher) {
    // generate packet vector
    let deduper = sigverify::Deduper::new(1_000_000);
    let mut count = 0;
    // verify packets
    let tx = test_tx();
    let mut batches = to_packet_batches(&std::iter::repeat(tx).take(NUM).collect::<Vec<_>>(), 128);
    bencher.iter(|| {
        let _ans = deduper.dedup_packets(&mut batches);
        batches
            .iter_mut()
            .for_each(|b| b.packets.iter_mut().for_each(|p| p.meta.discard = false));
        count += NUM;
    });
    println!("same total {}", count);
}

#[bench]
fn bench_dedup_diff(bencher: &mut Bencher) {
    // generate packet vector
    let deduper = sigverify::Deduper::new(1_000_000);

    let mut count = 0;
    let mut batches = to_packet_batches(&(0..NUM).map(|_| test_tx()).collect::<Vec<_>>(), 128);
    bencher.iter(|| {
        let _ans = deduper.dedup_packets(&mut batches);
        batches
            .iter_mut()
            .for_each(|b| b.packets.iter_mut().for_each(|p| p.meta.discard = false));
        count += NUM;
    });
    println!("diff total {}", count);
}

#[bench]
fn bench_dedup_baseline(bencher: &mut Bencher) {
    let mut count = 0;
    let mut batches = to_packet_batches(&(0..NUM).map(|_| test_tx()).collect::<Vec<_>>(), 128);
    bencher.iter(|| {
        batches
            .iter_mut()
            .for_each(|b| b.packets.iter_mut().for_each(|p| p.meta.discard = false));
        count += NUM;
    });
    println!("diff baseline total {}", count);
}

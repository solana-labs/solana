#![feature(test)]

extern crate test;

use {
    solana_bloom::bloom::{AtomicBloom, Bloom},
    solana_perf::{packet::to_packet_batches, sigverify, test_tx::test_tx},
    test::Bencher,
};

#[bench]
fn bench_dedup_same(bencher: &mut Bencher) {
    let tx = test_tx();

    // generate packet vector
    let mut batches = to_packet_batches(
        &std::iter::repeat(tx).take(64 * 1024).collect::<Vec<_>>(),
        128,
    );
    let packet_count = sigverify::count_packets_in_batches(&batches);
    let bloom: AtomicBloom<&[u8]> = Bloom::random(1_000_000, 0.0001, 8 << 22).into();

    println!("packet_count {} {}", packet_count, batches.len());

    // verify packets
    bencher.iter(|| {
        let _ans = sigverify::dedup_packets(&bloom, &mut batches);
    })
}

#[bench]
fn bench_dedup_diff(bencher: &mut Bencher) {
    // generate packet vector
    let mut batches =
        to_packet_batches(&(0..64 * 1024).map(|_| test_tx()).collect::<Vec<_>>(), 128);
    let packet_count = sigverify::count_packets_in_batches(&batches);
    let bloom: AtomicBloom<&[u8]> = Bloom::random(1_000_000, 0.0001, 8 << 22).into();

    println!("packet_count {} {}", packet_count, batches.len());

    // verify packets
    bencher.iter(|| {
        let _ans = sigverify::dedup_packets(&bloom, &mut batches);
    })
}

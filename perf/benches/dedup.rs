#![feature(test)]

extern crate test;

use {
    solana_bloom::bloom::{AtomicBloom, Bloom},
    solana_perf::{packet::to_packet_batches, sigverify, test_tx::test_tx},
    test::Bencher,
};

#[bench]
fn bench_dedup_same(bencher: &mut Bencher) {
    // generate packet vector
    let bloom: AtomicBloom<&[u8]> = Bloom::random(1_000_000, 0.0001, 8 << 22).into();
    let mut count = 0;
    // verify packets
    bencher.iter(|| {
        let tx = test_tx();
        let mut batches =
            to_packet_batches(&std::iter::repeat(tx).take(4096).collect::<Vec<_>>(), 128);
        let _ans = sigverify::dedup_packets(&bloom, &mut batches);
        count += 4096;
    });
    println!("same total {}", count);
}

#[bench]
fn bench_dedup_same_baseline(bencher: &mut Bencher) {
    let mut count = 0;
    bencher.iter(|| {
        let tx = test_tx();
        let mut batches =
            to_packet_batches(&std::iter::repeat(tx).take(4096).collect::<Vec<_>>(), 128);
        count += 4096;
    });
    println!("same baseline total {}", count);
}

#[bench]
fn bench_dedup_diff(bencher: &mut Bencher) {
    // generate packet vector
    let bloom: AtomicBloom<&[u8]> = Bloom::random(1_000_000, 0.0001, 8 << 22).into();

    let mut count = 0;
    // verify packets
    bencher.iter(|| {
        let mut batches = to_packet_batches(&(0..4096).map(|_| test_tx()).collect::<Vec<_>>(), 128);
        let _ans = sigverify::dedup_packets(&bloom, &mut batches);
        count += 4096;
    });
    println!("diff total {}", count);
}

#[bench]
fn bench_dedup_diff_baseline(bencher: &mut Bencher) {
    let mut count = 0;
    bencher.iter(|| {
        let mut _batches =
            to_packet_batches(&(0..4096).map(|_| test_tx()).collect::<Vec<_>>(), 128);
        count += 4096;
    });
    println!("diff baseline total {}", count);
}

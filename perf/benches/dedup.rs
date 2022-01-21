#![allow(clippy::integer_arithmetic)]
#![feature(test)]

extern crate test;

use {
    rand::prelude::*,
    solana_bloom::bloom::{AtomicBloom, Bloom},
    solana_perf::{
        packet::{to_packet_batches, PacketBatch},
        sigverify,
    },
    test::Bencher,
};

fn test_packet_with_size(size: usize, rng: &mut ThreadRng) -> Vec<u8> {
    // subtract 8 bytes because the length will get serialized as well
    (0..size.checked_sub(8).unwrap())
        .map(|_| rng.gen())
        .collect()
}

fn do_bench_dedup_packets(bencher: &mut Bencher, mut batches: Vec<PacketBatch>) {
    // verify packets
    let mut bloom: AtomicBloom<&[u8]> = Bloom::random(1_000_000, 0.0001, 8 << 22).into();
    bencher.iter(|| {
        // bench
        sigverify::dedup_packets(&bloom, &mut batches);

        // reset
        bloom.clear_for_tests();
        batches.iter_mut().for_each(|batch| {
            batch
                .packets
                .iter_mut()
                .for_each(|p| p.meta.set_discard(false))
        });
    })
}

#[bench]
#[ignore]
fn bench_dedup_same_small_packets(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let small_packet = test_packet_with_size(128, &mut rng);

    let batches = to_packet_batches(
        &std::iter::repeat(small_packet)
            .take(4096)
            .collect::<Vec<_>>(),
        128,
    );

    do_bench_dedup_packets(bencher, batches);
}

#[bench]
#[ignore]
fn bench_dedup_same_big_packets(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let big_packet = test_packet_with_size(1024, &mut rng);

    let batches = to_packet_batches(
        &std::iter::repeat(big_packet).take(4096).collect::<Vec<_>>(),
        128,
    );

    do_bench_dedup_packets(bencher, batches);
}

#[bench]
#[ignore]
fn bench_dedup_diff_small_packets(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();

    let batches = to_packet_batches(
        &(0..4096)
            .map(|_| test_packet_with_size(128, &mut rng))
            .collect::<Vec<_>>(),
        128,
    );

    do_bench_dedup_packets(bencher, batches);
}

#[bench]
#[ignore]
fn bench_dedup_diff_big_packets(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();

    let batches = to_packet_batches(
        &(0..4096)
            .map(|_| test_packet_with_size(1024, &mut rng))
            .collect::<Vec<_>>(),
        128,
    );

    do_bench_dedup_packets(bencher, batches);
}

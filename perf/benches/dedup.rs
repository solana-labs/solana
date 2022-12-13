#![allow(clippy::integer_arithmetic)]
#![feature(test)]

extern crate test;

use {
    rand::prelude::*,
    solana_perf::{
        packet::{to_packet_batches, Batch},
        sigverify,
    },
    solana_sdk::packet::Packet,
    std::time::Duration,
    test::Bencher,
};

const NUM: usize = 4096;

fn test_packet_with_size(size: usize, rng: &mut ThreadRng) -> Vec<u8> {
    // subtract 8 bytes because the length will get serialized as well
    (0..size.checked_sub(8).unwrap())
        .map(|_| rng.gen())
        .collect()
}

fn do_bench_dedup_packets<const N: usize>(bencher: &mut Bencher, mut batches: Vec<Batch<N>>) {
    // verify packets
    let mut deduper = sigverify::Deduper::new(1_000_000, Duration::from_millis(2_000));
    bencher.iter(|| {
        let _ans = deduper.dedup_packets_and_count_discards(&mut batches, |_, _, _| ());
        deduper.reset();
        batches
            .iter_mut()
            .for_each(|b| b.iter_mut().for_each(|p| p.meta_mut().set_discard(false)));
    });
}

#[bench]
#[ignore]
fn bench_dedup_same_small_packets(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let small_packet = test_packet_with_size(128, &mut rng);

    let batches = to_packet_batches::<{ Packet::DATA_SIZE }, _>(
        &std::iter::repeat(small_packet)
            .take(NUM)
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

    let batches = to_packet_batches::<{ Packet::DATA_SIZE }, _>(
        &std::iter::repeat(big_packet).take(NUM).collect::<Vec<_>>(),
        128,
    );

    do_bench_dedup_packets(bencher, batches);
}

#[bench]
#[ignore]
fn bench_dedup_diff_small_packets(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();

    let batches = to_packet_batches::<{ Packet::DATA_SIZE }, _>(
        &(0..NUM)
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

    let batches = to_packet_batches::<{ Packet::DATA_SIZE }, _>(
        &(0..NUM)
            .map(|_| test_packet_with_size(1024, &mut rng))
            .collect::<Vec<_>>(),
        128,
    );

    do_bench_dedup_packets(bencher, batches);
}

#[bench]
#[ignore]
fn bench_dedup_baseline(bencher: &mut Bencher) {
    let mut rng = rand::thread_rng();

    let batches = to_packet_batches::<{ Packet::DATA_SIZE }, _>(
        &(0..0)
            .map(|_| test_packet_with_size(128, &mut rng))
            .collect::<Vec<_>>(),
        128,
    );

    do_bench_dedup_packets(bencher, batches);
}

#[bench]
#[ignore]
fn bench_dedup_reset(bencher: &mut Bencher) {
    let mut deduper = sigverify::Deduper::new(1_000_000, Duration::from_millis(0));
    bencher.iter(|| {
        deduper.reset();
    });
}

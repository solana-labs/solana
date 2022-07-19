#![allow(clippy::integer_arithmetic)]
#![feature(test)]

extern crate test;

use {
    rand::distributions::{Distribution, Uniform},
    solana_core::{
        banking_stage::*, forward_packet_batches_by_accounts::ForwardPacketBatchesByAccounts,
        unprocessed_packet_batches::*,
    },
    solana_measure::measure::Measure,
    solana_perf::packet::{Packet, PacketBatch},
    solana_runtime::{
        bank::Bank,
        bank_forks::BankForks,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
    },
    solana_sdk::{hash::Hash, signature::Keypair, system_transaction},
    std::sync::{Arc, RwLock},
    test::Bencher,
};

fn build_packet_batch(packet_per_batch_count: usize) -> (PacketBatch, Vec<usize>) {
    let packet_batch = PacketBatch::new(
        (0..packet_per_batch_count)
            .map(|sender_stake| {
                let tx = system_transaction::transfer(
                    &Keypair::new(),
                    &solana_sdk::pubkey::new_rand(),
                    1,
                    Hash::new_unique(),
                );
                let mut packet = Packet::from_data(None, &tx).unwrap();
                packet.meta.sender_stake = sender_stake as u64;
                packet
            })
            .collect(),
    );
    let packet_indexes: Vec<usize> = (0..packet_per_batch_count).collect();

    (packet_batch, packet_indexes)
}

fn build_randomized_packet_batch(packet_per_batch_count: usize) -> (PacketBatch, Vec<usize>) {
    let mut rng = rand::thread_rng();
    let distribution = Uniform::from(0..200_000);

    let packet_batch = PacketBatch::new(
        (0..packet_per_batch_count)
            .map(|_| {
                let tx = system_transaction::transfer(
                    &Keypair::new(),
                    &solana_sdk::pubkey::new_rand(),
                    1,
                    Hash::new_unique(),
                );
                let mut packet = Packet::from_data(None, &tx).unwrap();
                let sender_stake = distribution.sample(&mut rng);
                packet.meta.sender_stake = sender_stake as u64;
                packet
            })
            .collect(),
    );
    let packet_indexes: Vec<usize> = (0..packet_per_batch_count).collect();

    (packet_batch, packet_indexes)
}

fn insert_packet_batches(
    buffer_max_size: usize,
    batch_count: usize,
    packet_per_batch_count: usize,
    randomize: bool,
) {
    solana_logger::setup();
    let mut unprocessed_packet_batches = UnprocessedPacketBatches::with_capacity(buffer_max_size);

    let mut timer = Measure::start("insert_batch");
    (0..batch_count).for_each(|_| {
        let (packet_batch, packet_indexes) = if randomize {
            build_randomized_packet_batch(packet_per_batch_count)
        } else {
            build_packet_batch(packet_per_batch_count)
        };
        let deserialized_packets = deserialize_packets(&packet_batch, &packet_indexes);
        unprocessed_packet_batches.insert_batch(deserialized_packets);
    });
    timer.stop();
    log::info!(
        "inserted {} batch, elapsed {}",
        buffer_max_size,
        timer.as_us()
    );
}

#[bench]
#[allow(clippy::unit_arg)]
fn bench_packet_clone(bencher: &mut Bencher) {
    let batch_count = 1000;
    let packet_per_batch_count = 128;

    let packet_batches: Vec<PacketBatch> = (0..batch_count)
        .map(|_| build_packet_batch(packet_per_batch_count).0)
        .collect();

    bencher.iter(|| {
        test::black_box(packet_batches.iter().for_each(|packet_batch| {
            let mut outer_packet = Packet::default();

            let mut timer = Measure::start("insert_batch");
            packet_batch.iter().for_each(|packet| {
                let mut packet = packet.clone();
                packet.meta.sender_stake *= 2;
                if packet.meta.sender_stake > 2 {
                    outer_packet = packet;
                }
            });

            timer.stop();
        }));
    });
}

//*
// v1, bench: 5,600,038,163 ns/iter (+/- 940,818,988)
// v2, bench: 5,265,382,750 ns/iter (+/- 153,623,264)
#[bench]
#[ignore]
fn bench_unprocessed_packet_batches_within_limit(bencher: &mut Bencher) {
    let buffer_capacity = 1_000 * 128;
    let batch_count = 1_000;
    let packet_per_batch_count = 128;

    bencher.iter(|| {
        insert_packet_batches(buffer_capacity, batch_count, packet_per_batch_count, false);
    });
}

// v1, bench: 6,607,014,940 ns/iter (+/- 768,191,361)
// v2, bench: 5,692,753,323 ns/iter (+/- 548,959,624)
#[bench]
#[ignore]
fn bench_unprocessed_packet_batches_beyond_limit(bencher: &mut Bencher) {
    let buffer_capacity = 1_000 * 128;
    let batch_count = 1_100;
    let packet_per_batch_count = 128;

    // this is the worst scenario testing: all batches are uniformly populated with packets from
    // priority 100..228, so in order to drop a batch, algo will have to drop all packets that has
    // priority < 228, plus one 228. That's 2000 batch * 127 packets + 1
    // Also, since all batches have same stake distribution, the new one is always the one got
    // dropped. Tho it does not change algo complexity.
    bencher.iter(|| {
        insert_packet_batches(buffer_capacity, batch_count, packet_per_batch_count, false);
    });
}
// */
// v1, bench: 5,843,307,086 ns/iter (+/- 844,249,298)
// v2, bench: 5,139,525,951 ns/iter (+/- 48,005,521)
#[bench]
#[ignore]
fn bench_unprocessed_packet_batches_randomized_within_limit(bencher: &mut Bencher) {
    let buffer_capacity = 1_000 * 128;
    let batch_count = 1_000;
    let packet_per_batch_count = 128;

    bencher.iter(|| {
        insert_packet_batches(buffer_capacity, batch_count, packet_per_batch_count, true);
    });
}

// v1, bench: 6,497,623,849 ns/iter (+/- 3,206,382,212)
// v2, bench: 5,762,071,682 ns/iter (+/- 168,244,418)
#[bench]
#[ignore]
fn bench_unprocessed_packet_batches_randomized_beyond_limit(bencher: &mut Bencher) {
    let buffer_capacity = 1_000 * 128;
    let batch_count = 1_100;
    let packet_per_batch_count = 128;

    bencher.iter(|| {
        insert_packet_batches(buffer_capacity, batch_count, packet_per_batch_count, true);
    });
}

fn build_bank_forks_for_test() -> Arc<RwLock<BankForks>> {
    let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
    let bank = Bank::new_for_tests(&genesis_config);
    let bank_forks = BankForks::new(bank);
    Arc::new(RwLock::new(bank_forks))
}

fn buffer_iter_desc_and_forward(
    buffer_max_size: usize,
    batch_count: usize,
    packet_per_batch_count: usize,
    randomize: bool,
) {
    solana_logger::setup();
    let mut unprocessed_packet_batches = UnprocessedPacketBatches::with_capacity(buffer_max_size);

    // fill buffer
    {
        let mut timer = Measure::start("fill_buffer");
        (0..batch_count).for_each(|_| {
            let (packet_batch, packet_indexes) = if randomize {
                build_randomized_packet_batch(packet_per_batch_count)
            } else {
                build_packet_batch(packet_per_batch_count)
            };
            let deserialized_packets = deserialize_packets(&packet_batch, &packet_indexes);
            unprocessed_packet_batches.insert_batch(deserialized_packets);
        });
        timer.stop();
        log::info!(
            "inserted {} batch, elapsed {}",
            buffer_max_size,
            timer.as_us()
        );
    }

    // forward whole buffer
    {
        let mut timer = Measure::start("forward_time");
        let mut forward_packet_batches_by_accounts =
            ForwardPacketBatchesByAccounts::new_with_default_batch_limits(
                build_bank_forks_for_test().read().unwrap().root_bank(),
            );
        // iter_desc buffer
        let filter_forwarding_results = BankingStage::filter_valid_packets_for_forwarding(
            &mut unprocessed_packet_batches,
            &mut forward_packet_batches_by_accounts,
        );
        timer.stop();

        let batched_filter_forwarding_results: usize = forward_packet_batches_by_accounts
            .iter_batches()
            .map(|forward_batch| forward_batch.len())
            .sum();
        log::info!(
            "filter_forwarding_results {:?}, batched_forwardable packets {}, elapsed {}",
            filter_forwarding_results,
            batched_filter_forwarding_results,
            timer.as_us()
        );
    }
}

#[bench]
#[ignore]
fn bench_forwarding_unprocessed_packet_batches(bencher: &mut Bencher) {
    let batch_count = 1_000;
    let packet_per_batch_count = 64;
    let buffer_capacity = batch_count * packet_per_batch_count;

    bencher.iter(|| {
        buffer_iter_desc_and_forward(buffer_capacity, batch_count, packet_per_batch_count, true);
    });
}

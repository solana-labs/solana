#![feature(test)]
#![allow(clippy::arithmetic_side_effects)]

extern crate solana_core;
extern crate test;

use {
    crossbeam_channel::unbounded,
    log::*,
    rand::{
        distributions::{Distribution, Uniform},
        thread_rng, Rng,
    },
    solana_core::{
        banking_trace::BankingTracer,
        sigverify::TransactionSigVerifier,
        sigverify_stage::{SigVerifier, SigVerifyStage},
    },
    solana_measure::measure::Measure,
    solana_perf::{
        packet::{to_packet_batches, PacketBatch},
        test_tx::test_tx,
    },
    solana_sdk::{
        hash::Hash,
        packet::PacketFlags,
        signature::{Keypair, Signer},
        system_transaction,
        timing::duration_as_ms,
    },
    std::time::{Duration, Instant},
    test::Bencher,
};

fn run_bench_packet_discard(num_ips: usize, bencher: &mut Bencher) {
    solana_logger::setup();
    let len = 30 * 1000;
    let chunk_size = 1024;
    let tx = test_tx();
    let mut batches = to_packet_batches(&vec![tx; len], chunk_size);

    let mut total = 0;

    let ips: Vec<_> = (0..num_ips)
        .map(|_| {
            let mut addr = [0u16; 8];
            thread_rng().fill(&mut addr);
            std::net::IpAddr::from(addr)
        })
        .collect();

    for batch in batches.iter_mut() {
        total += batch.len();
        for p in batch.iter_mut() {
            let ip_index = thread_rng().gen_range(0..ips.len());
            p.meta_mut().addr = ips[ip_index];
        }
    }
    info!("total packets: {}", total);

    bencher.iter(move || {
        SigVerifyStage::discard_excess_packets(&mut batches, 10_000, |_| ());
        let mut num_packets = 0;
        for batch in batches.iter_mut() {
            for p in batch.iter_mut() {
                if !p.meta().discard() {
                    num_packets += 1;
                }
                p.meta_mut().set_discard(false);
            }
        }
        assert_eq!(num_packets, 10_000);
    });
}

#[bench]
fn bench_packet_discard_many_senders(bencher: &mut Bencher) {
    run_bench_packet_discard(1000, bencher);
}

#[bench]
fn bench_packet_discard_single_sender(bencher: &mut Bencher) {
    run_bench_packet_discard(1, bencher);
}

#[bench]
fn bench_packet_discard_mixed_senders(bencher: &mut Bencher) {
    const SIZE: usize = 30 * 1000;
    const CHUNK_SIZE: usize = 1024;
    fn new_rand_addr<R: Rng>(rng: &mut R) -> std::net::IpAddr {
        let mut addr = [0u16; 8];
        rng.fill(&mut addr);
        std::net::IpAddr::from(addr)
    }
    let mut rng = thread_rng();
    let mut batches = to_packet_batches(&vec![test_tx(); SIZE], CHUNK_SIZE);
    let spam_addr = new_rand_addr(&mut rng);
    for batch in batches.iter_mut() {
        for packet in batch.iter_mut() {
            // One spam address, ~1000 unique addresses.
            packet.meta_mut().addr = if rng.gen_ratio(1, 30) {
                new_rand_addr(&mut rng)
            } else {
                spam_addr
            }
        }
    }
    bencher.iter(move || {
        SigVerifyStage::discard_excess_packets(&mut batches, 10_000, |_| ());
        let mut num_packets = 0;
        for batch in batches.iter_mut() {
            for packet in batch.iter_mut() {
                if !packet.meta().discard() {
                    num_packets += 1;
                }
                packet.meta_mut().set_discard(false);
            }
        }
        assert_eq!(num_packets, 10_000);
    });
}

fn gen_batches(use_same_tx: bool) -> Vec<PacketBatch> {
    let len = 4096;
    let chunk_size = 1024;
    if use_same_tx {
        let tx = test_tx();
        to_packet_batches(&vec![tx; len], chunk_size)
    } else {
        let from_keypair = Keypair::new();
        let to_keypair = Keypair::new();
        let txs: Vec<_> = (0..len)
            .map(|_| {
                let amount = thread_rng().gen();
                system_transaction::transfer(
                    &from_keypair,
                    &to_keypair.pubkey(),
                    amount,
                    Hash::default(),
                )
            })
            .collect();
        to_packet_batches(&txs, chunk_size)
    }
}

#[bench]
fn bench_sigverify_stage_with_same_tx(bencher: &mut Bencher) {
    bench_sigverify_stage(bencher, true)
}

#[bench]
fn bench_sigverify_stage_without_same_tx(bencher: &mut Bencher) {
    bench_sigverify_stage(bencher, false)
}

fn bench_sigverify_stage(bencher: &mut Bencher, use_same_tx: bool) {
    solana_logger::setup();
    trace!("start");
    let (packet_s, packet_r) = unbounded();
    let (verified_s, verified_r) = BankingTracer::channel_for_test();
    let verifier = TransactionSigVerifier::new(verified_s);
    let stage = SigVerifyStage::new(packet_r, verifier, "bench");

    bencher.iter(move || {
        let now = Instant::now();
        let batches = gen_batches(use_same_tx);
        trace!(
            "starting... generation took: {} ms batches: {}",
            duration_as_ms(&now.elapsed()),
            batches.len()
        );

        let mut sent_len = 0;
        for mut batch in batches.into_iter() {
            sent_len += batch.len();
            batch
                .iter_mut()
                .for_each(|packet| packet.meta_mut().flags |= PacketFlags::TRACER_PACKET);
            packet_s.send(batch).unwrap();
        }
        let mut received = 0;
        let mut total_tracer_packets_received_in_sigverify_stage = 0;
        trace!("sent: {}", sent_len);
        loop {
            if let Ok(message) = verified_r.recv_timeout(Duration::from_millis(10)) {
                let (verifieds, tracer_packet_stats) = (&message.0, message.1.as_ref().unwrap());
                received += verifieds.iter().map(|batch| batch.len()).sum::<usize>();
                total_tracer_packets_received_in_sigverify_stage +=
                    tracer_packet_stats.total_tracer_packets_received_in_sigverify_stage;
                test::black_box(message);
                if total_tracer_packets_received_in_sigverify_stage >= sent_len {
                    break;
                }
            }
        }
        trace!("received: {}", received);
    });
    stage.join().unwrap();
}

fn prepare_batches(discard_factor: i32) -> (Vec<PacketBatch>, usize) {
    let len = 10_000; // max batch size
    let chunk_size = 1024;

    let from_keypair = Keypair::new();
    let to_keypair = Keypair::new();

    let txs: Vec<_> = (0..len)
        .map(|_| {
            let amount = thread_rng().gen();
            system_transaction::transfer(
                &from_keypair,
                &to_keypair.pubkey(),
                amount,
                Hash::default(),
            )
        })
        .collect();
    let mut batches = to_packet_batches(&txs, chunk_size);

    let mut rng = rand::thread_rng();
    let die = Uniform::<i32>::from(1..100);

    let mut c = 0;
    batches.iter_mut().for_each(|batch| {
        batch.iter_mut().for_each(|p| {
            let throw = die.sample(&mut rng);
            if throw < discard_factor {
                p.meta_mut().set_discard(true);
                c += 1;
            }
        })
    });
    (batches, len - c)
}

fn bench_shrink_sigverify_stage_core(bencher: &mut Bencher, discard_factor: i32) {
    let (batches0, num_valid_packets) = prepare_batches(discard_factor);
    let (verified_s, _verified_r) = BankingTracer::channel_for_test();
    let verifier = TransactionSigVerifier::new(verified_s);

    let mut c = 0;
    let mut total_shrink_time = 0;
    let mut total_verify_time = 0;

    bencher.iter(|| {
        let mut batches = batches0.clone();
        let (pre_shrink_time_us, _pre_shrink_total) =
            SigVerifyStage::maybe_shrink_batches(&mut batches);

        let mut verify_time = Measure::start("sigverify_batch_time");
        let _batches = verifier.verify_batches(batches, num_valid_packets);
        verify_time.stop();

        c += 1;
        total_shrink_time += pre_shrink_time_us;
        total_verify_time += verify_time.as_us();
    });

    error!(
        "bsv, {}, {}, {}",
        discard_factor,
        (total_shrink_time as f64) / (c as f64),
        (total_verify_time as f64) / (c as f64),
    );
}

macro_rules! GEN_SHRINK_SIGVERIFY_BENCH {
    ($i:ident, $n:literal) => {
        #[bench]
        fn $i(bencher: &mut Bencher) {
            bench_shrink_sigverify_stage_core(bencher, $n);
        }
    };
}

GEN_SHRINK_SIGVERIFY_BENCH!(bsv_0, 0);
GEN_SHRINK_SIGVERIFY_BENCH!(bsv_10, 10);
GEN_SHRINK_SIGVERIFY_BENCH!(bsv_20, 20);
GEN_SHRINK_SIGVERIFY_BENCH!(bsv_30, 30);
GEN_SHRINK_SIGVERIFY_BENCH!(bsv_40, 40);
GEN_SHRINK_SIGVERIFY_BENCH!(bsv_50, 50);
GEN_SHRINK_SIGVERIFY_BENCH!(bsv_60, 60);
GEN_SHRINK_SIGVERIFY_BENCH!(bsv_70, 70);
GEN_SHRINK_SIGVERIFY_BENCH!(bsv_80, 80);
GEN_SHRINK_SIGVERIFY_BENCH!(bsv_90, 90);

#![feature(test)]
#![allow(clippy::integer_arithmetic)]

extern crate solana_core;
extern crate test;

use {
    crossbeam_channel::unbounded,
    log::*,
    rand::{thread_rng, Rng},
    solana_core::{sigverify::TransactionSigVerifier, sigverify_stage::SigVerifyStage},
    solana_perf::{packet::to_packet_batches, test_tx::test_tx},
    solana_sdk::{
        hash::Hash,
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
        .into_iter()
        .map(|_| {
            let mut addr = [0u16; 8];
            thread_rng().fill(&mut addr);
            std::net::IpAddr::from(addr)
        })
        .collect();

    for batch in batches.iter_mut() {
        total += batch.packets.len();
        for p in batch.packets.iter_mut() {
            let ip_index = thread_rng().gen_range(0, ips.len());
            p.meta.addr = ips[ip_index];
        }
    }
    info!("total packets: {}", total);

    bencher.iter(move || {
        SigVerifyStage::discard_excess_packets(&mut batches, 10_000);
        let mut num_packets = 0;
        for batch in batches.iter_mut() {
            for p in batch.packets.iter_mut() {
                if !p.meta.discard() {
                    num_packets += 1;
                }
                p.meta.set_discard(false);
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
        for packet in batch.packets.iter_mut() {
            // One spam address, ~1000 unique addresses.
            packet.meta.addr = if rng.gen_ratio(1, 30) {
                new_rand_addr(&mut rng)
            } else {
                spam_addr
            }
        }
    }
    bencher.iter(move || {
        SigVerifyStage::discard_excess_packets(&mut batches, 10_000);
        let mut num_packets = 0;
        for batch in batches.iter_mut() {
            for packet in batch.packets.iter_mut() {
                if !packet.meta.discard() {
                    num_packets += 1;
                }
                packet.meta.set_discard(false);
            }
        }
        assert_eq!(num_packets, 10_000);
    });
}

#[bench]
fn bench_sigverify_stage(bencher: &mut Bencher) {
    solana_logger::setup();
    let (packet_s, packet_r) = unbounded();
    let (verified_s, verified_r) = unbounded();
    let verifier = TransactionSigVerifier::default();
    let stage = SigVerifyStage::new(packet_r, verified_s, verifier);

    let now = Instant::now();
    let len = 4096;
    let use_same_tx = true;
    let chunk_size = 1024;
    let mut batches = if use_same_tx {
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
    };

    trace!(
        "starting... generation took: {} ms batches: {}",
        duration_as_ms(&now.elapsed()),
        batches.len()
    );
    bencher.iter(move || {
        let mut sent_len = 0;
        for _ in 0..batches.len() {
            if let Some(batch) = batches.pop() {
                sent_len += batch.packets.len();
                packet_s.send(batch).unwrap();
            }
        }
        let mut received = 0;
        trace!("sent: {}", sent_len);
        loop {
            if let Ok(mut verifieds) = verified_r.recv_timeout(Duration::from_millis(10)) {
                while let Some(v) = verifieds.pop() {
                    received += v.packets.len();
                    batches.push(v);
                }
                if received >= sent_len {
                    break;
                }
            }
        }
        trace!("received: {}", received);
    });
    stage.join().unwrap();
}

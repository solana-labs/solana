#![feature(test)]
#![allow(clippy::integer_arithmetic)]

extern crate solana_core;
extern crate test;

use {
    crossbeam_channel::unbounded,
    log::*,
    rand::{thread_rng, Rng},
    solana_core::{sigverify::TransactionSigVerifier, sigverify_stage::SigVerifyStage},
    solana_perf::{
        packet::{to_packet_batches, PacketBatch},
        sigverify::discard_excess_packets,
        test_tx::test_tx,
    },
    solana_sdk::{
        hash::Hash,
        signature::{Keypair, Signer},
        system_transaction,
        system_instruction,
        message::Message,
        transaction::Transaction,
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
        discard_excess_packets(&mut batches, 10_000);
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
        discard_excess_packets(&mut batches, 10_000);
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
fn bench_sigverify_stage(bencher: &mut Bencher) {
    solana_logger::setup();
    trace!("start");
    let (packet_s, packet_r) = unbounded();
    let (verified_s, verified_r) = unbounded();
    let verifier = TransactionSigVerifier::default();
    let stage = SigVerifyStage::new(packet_r, verified_s, verifier, "bench");

    let use_same_tx = true;
    bencher.iter(move || {
        let now = Instant::now();
        let mut batches = gen_batches(use_same_tx);
        trace!(
            "starting... generation took: {} ms batches: {}",
            duration_as_ms(&now.elapsed()),
            batches.len()
        );

        let mut sent_len = 0;
        for _ in 0..batches.len() {
            if let Some(batch) = batches.pop() {
                sent_len += batch.packets.len();
                packet_s.send(vec![batch]).unwrap();
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
                if use_same_tx || received >= sent_len {
                    break;
                }
            }
        }
        trace!("received: {}", received);
    });
    stage.join().unwrap();
}


fn gen_dup_and_valid(offset: usize, num_packets: usize, num_batches: usize, gen_valid: usize, gen_dups: usize) -> Vec<PacketBatch> {
    let chunk_size = num_packets / num_batches;
    let from_keypair = Keypair::new();
    let from_pubkey = from_keypair.pubkey();
    let to_keypair = Keypair::new();
    let to_pubkey = to_keypair.pubkey();

    // dedup scales with packet size, use a reasonably sized tx
    // this is 1048 bytes, instead of 215 bytes that a single transfer ix would be
    let dup_tx = {
        let instructions = (0..50).map(|i| system_instruction::transfer(&from_pubkey, &to_pubkey, i)).collect::<Vec<_>>();
        let message = Message::new(&instructions, Some(&from_pubkey));
        Transaction::new(&[&from_keypair], message, Hash::default())
    };

    let txs: Vec<_> = (offset..offset + num_packets)
        .map(|i| {
            let tx_type = i % (gen_valid + gen_dups);
            if tx_type < gen_valid {
                let amount = i as u64;
                system_transaction::transfer(
                    &from_keypair,
                    &to_pubkey,
                    amount,
                    Hash::default(),
                )
            } else {
                dup_tx.clone()
            }
        })
        .collect();
    to_packet_batches(&txs, chunk_size)
}

#[bench]
fn bench_sigverify_stage2(bencher: &mut Bencher) {
    use {
        std::time::{Instant, Duration},
        std::thread,
    };

    solana_logger::setup();
    trace!("start");
    let (packet_s, packet_r) = unbounded();
    let (verified_s, verified_r) = unbounded();
    let verifier = TransactionSigVerifier::default();
    let stage = SigVerifyStage::new(packet_r, verified_s, verifier, "bench");

    warn!("gen");
    let packets_per_sec = 1_500_000;
    let batches_per_sec = 1000;
    let calls_per_sec: usize = 100;
    let packets_per_call = packets_per_sec / calls_per_sec;
    let batches_per_call = batches_per_sec / calls_per_sec;
    let num_secs = 4;
    let chunked_tx = (0..num_secs * calls_per_sec).map(|i| gen_dup_and_valid(i * packets_per_call, packets_per_call, batches_per_call, 1, 9)).collect::<Vec<Vec<_>>>();
    warn!("gen done {} {}", chunked_tx[0][0].packets[0].meta.size, chunked_tx[0][0].packets[1].meta.size);

    // start a thread to feed batches every 10ms
    thread::spawn(move || {
        let wait_ms = Duration::from_millis(1000 / calls_per_sec as u64);
        let mut last_send = Instant::now();
        for batches in chunked_tx {
            last_send = Instant::now();
            packet_s.send(batches);
            let elapsed = last_send.elapsed();
            if elapsed < wait_ms {
                thread::sleep(wait_ms - elapsed);
            }
        }
    });

    // observe the output

    stage.join().unwrap();
}

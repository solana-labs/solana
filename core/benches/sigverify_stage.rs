#![feature(test)]

extern crate solana_core;
extern crate test;

use crossbeam_channel::unbounded;
use log::*;
use rand::{thread_rng, Rng};
use solana_core::packet::to_packets_chunked;
use solana_core::service::Service;
use solana_core::sigverify::TransactionSigVerifier;
use solana_core::sigverify_stage::SigVerifyStage;
use solana_perf::test_tx::test_tx;
use solana_sdk::hash::Hash;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction;
use solana_sdk::timing::duration_as_ms;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};
use test::Bencher;

#[bench]
fn bench_sigverify_stage(bencher: &mut Bencher) {
    solana_logger::setup();
    let (packet_s, packet_r) = channel();
    let (verified_s, verified_r) = unbounded();
    let verifier = TransactionSigVerifier::default();
    let stage = SigVerifyStage::new(packet_r, verified_s, verifier);

    let now = Instant::now();
    let len = 4096;
    let use_same_tx = true;
    let chunk_size = 1024;
    let mut batches = if use_same_tx {
        let tx = test_tx();
        to_packets_chunked(&vec![tx; len], chunk_size)
    } else {
        let from_keypair = Keypair::new();
        let to_keypair = Keypair::new();
        let txs: Vec<_> = (0..len)
            .into_iter()
            .map(|_| {
                let amount = thread_rng().gen();
                let tx = system_transaction::transfer(
                    &from_keypair,
                    &to_keypair.pubkey(),
                    amount,
                    Hash::default(),
                );
                tx
            })
            .collect();
        to_packets_chunked(&txs, chunk_size)
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

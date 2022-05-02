#![feature(test)]
#![allow(clippy::integer_arithmetic)]

extern crate solana_core;
extern crate test;

use {
    crossbeam_channel::unbounded,
    log::*,
    rand::{thread_rng, Rng},
    solana_core::find_packet_sender_stake_stage::FindPacketSenderStakeStage,
    solana_gossip::cluster_info::{ClusterInfo, Node},
    solana_ledger::genesis_utils::{create_genesis_config, GenesisConfigInfo},
    solana_perf::{
        packet::{to_packet_batches, PacketBatch},
        test_tx::test_tx,
    },
    solana_runtime::{bank::Bank, bank_forks::BankForks},
    solana_sdk::{
        hash::Hash,
        pubkey,
        signature::{Keypair, Signer},
        system_transaction,
        timing::duration_as_ms,
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        sync::{Arc, RwLock},
        time::{Duration, Instant},
    },
    test::Bencher,
};

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
fn bench_dedup_stage(bencher: &mut Bencher) {
    solana_logger::setup();
    trace!("start");
    let leader_pubkey = pubkey::new_rand();
    let leader_info = Node::new_localhost_with_pubkey(&leader_pubkey);
    let cluster_info = Arc::new(ClusterInfo::new(
        leader_info.info,
        Arc::new(Keypair::new()),
        SocketAddrSpace::Unspecified,
    ));

    let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
    let bank = Bank::new_for_benches(&genesis_config);
    let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));

    let (packet_s, packet_r) = unbounded();
    let (verified_s, verified_r) = unbounded();
    let stage = FindPacketSenderStakeStage::new(packet_r, verified_s, bank_forks, cluster_info);

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

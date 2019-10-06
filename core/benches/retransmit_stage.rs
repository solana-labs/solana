#![feature(test)]

extern crate solana_core;
extern crate test;

use log::*;
use solana_core::bank_forks::BankForks;
use solana_core::cluster_info::{ClusterInfo, Node};
use solana_core::contact_info::ContactInfo;
use solana_core::genesis_utils::{create_genesis_block, GenesisBlockInfo};
use solana_core::leader_schedule_cache::LeaderScheduleCache;
use solana_core::packet::to_packets_chunked;
use solana_core::retransmit_stage::retransmit;
use solana_core::test_tx::test_tx;
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::timestamp;
use std::net::UdpSocket;
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use test::Bencher;

#[bench]
fn bench_retransmit(bencher: &mut Bencher) {
    solana_logger::setup();
    let mut cluster_info = ClusterInfo::new_with_invalid_keypair(Node::new_localhost().info);
    const NUM_PEERS: usize = 2;
    for _ in 0..NUM_PEERS {
        let id = Pubkey::new_rand();
        let contact_info = ContactInfo::new_localhost(&id, timestamp());
        cluster_info.insert_info(contact_info);
    }
    let cluster_info = Arc::new(RwLock::new(cluster_info));

    let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(100_000);
    let bank0 = Bank::new(&genesis_block);
    let bank_forks = BankForks::new(0, bank0);
    let bank = bank_forks.working_bank();
    let bank_forks = Arc::new(RwLock::new(bank_forks));
    let (packet_sender, packet_receiver) = channel();
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    socket.set_nonblocking(true).unwrap();

    let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));

    let tx = test_tx();
    let len = 4096;
    let chunk_size = 1024;
    let batches = to_packets_chunked(&vec![tx; len], chunk_size);

    bencher.iter(move || {
        for packets in batches.clone() {
            packet_sender.send(packets).unwrap();
        }
        info!("sent...");
        retransmit(
            &bank_forks,
            &leader_schedule_cache,
            &cluster_info,
            &packet_receiver,
            &socket,
        )
        .unwrap();
    });
}

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
use solana_core::retransmit_stage::retransmitter;
use solana_core::test_tx::test_tx;
use solana_measure::measure::Measure;
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::timestamp;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::Mutex;
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::thread::Builder;
use std::time::Duration;
use test::Bencher;

#[bench]
fn bench_retransmitter(bencher: &mut Bencher) {
    solana_logger::setup();
    let mut cluster_info = ClusterInfo::new_with_invalid_keypair(Node::new_localhost().info);
    const NUM_PEERS: usize = 4;
    let mut peer_sockets = Vec::new();
    for _ in 0..NUM_PEERS {
        let id = Pubkey::new_rand();
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut contact_info = ContactInfo::new_localhost(&id, timestamp());
        contact_info.tvu = socket.local_addr().unwrap();
        contact_info.tvu.set_ip("127.0.0.1".parse().unwrap());
        contact_info.tvu_forwards = contact_info.tvu;
        info!("local: {:?}", contact_info.tvu);
        cluster_info.insert_info(contact_info);
        socket.set_nonblocking(true).unwrap();
        peer_sockets.push(socket);
    }
    let peer_sockets = Arc::new(peer_sockets);
    let cluster_info = Arc::new(RwLock::new(cluster_info));

    let GenesisBlockInfo { genesis_block, .. } = create_genesis_block(100_000);
    let bank0 = Bank::new(&genesis_block);
    let bank_forks = BankForks::new(0, bank0);
    let bank = bank_forks.working_bank();
    let bank_forks = Arc::new(RwLock::new(bank_forks));
    let (packet_sender, packet_receiver) = channel();
    let packet_receiver = Arc::new(Mutex::new(packet_receiver));
    const NUM_THREADS: usize = 2;
    let sockets = (0..NUM_THREADS)
        .map(|_| UdpSocket::bind("0.0.0.0:0").unwrap())
        .collect();

    let leader_schedule_cache = Arc::new(LeaderScheduleCache::new_from_bank(&bank));

    // To work reliably with higher values, this needs larger udp rmem size
    let tx = test_tx();
    const NUM_PACKETS: usize = 50;
    let chunk_size = NUM_PACKETS / (4 * NUM_THREADS);
    let batches = to_packets_chunked(&vec![tx; NUM_PACKETS], chunk_size);
    info!("batches: {}", batches.len());

    let retransmitter_handles = retransmitter(
        Arc::new(sockets),
        bank_forks,
        &leader_schedule_cache,
        cluster_info,
        packet_receiver,
    );

    let total = Arc::new(AtomicUsize::new(0));
    bencher.iter(move || {
        let peer_sockets1 = peer_sockets.clone();
        let handles: Vec<_> = (0..NUM_PEERS)
            .into_iter()
            .map(|p| {
                let peer_sockets2 = peer_sockets1.clone();
                let total2 = total.clone();
                Builder::new()
                    .name("recv".to_string())
                    .spawn(move || {
                        info!("{} waiting on {:?}", p, peer_sockets2[p]);
                        let mut buf = [0u8; 1024];
                        loop {
                            while peer_sockets2[p].recv(&mut buf).is_ok() {
                                total2.fetch_add(1, Ordering::Relaxed);
                            }
                            if total2.load(Ordering::Relaxed) >= NUM_PACKETS {
                                break;
                            }
                            info!("{} recv", total2.load(Ordering::Relaxed));
                            sleep(Duration::from_millis(1));
                        }
                    })
                    .unwrap()
            })
            .collect();

        for packets in batches.clone() {
            packet_sender.send(packets).unwrap();
        }
        info!("sent...");

        let mut join_time = Measure::start("join");
        for h in handles {
            h.join().unwrap();
        }
        join_time.stop();
        info!("took: {}ms", join_time.as_ms());

        total.store(0, Ordering::Relaxed);
    });

    for t in retransmitter_handles {
        t.join().unwrap();
    }
}

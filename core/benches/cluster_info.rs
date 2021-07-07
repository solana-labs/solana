#![feature(test)]

extern crate test;

use rand::{thread_rng, Rng};
use solana_core::{
    broadcast_stage::{broadcast_metrics::TransmitShredsStats, broadcast_shreds, BroadcastStage},
    cluster_nodes::ClusterNodes,
};
use solana_gossip::{
    cluster_info::{ClusterInfo, Node},
    contact_info::ContactInfo,
};
use solana_ledger::shred::Shred;
use solana_sdk::pubkey;
use solana_sdk::timing::timestamp;
use std::{
    collections::HashMap,
    net::UdpSocket,
    sync::{atomic::AtomicU64, Arc},
};
use test::Bencher;

#[bench]
fn broadcast_shreds_bench(bencher: &mut Bencher) {
    solana_logger::setup();
    let leader_pubkey = pubkey::new_rand();
    let leader_info = Node::new_localhost_with_pubkey(&leader_pubkey);
    let cluster_info = ClusterInfo::new_with_invalid_keypair(leader_info.info);
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

    const NUM_SHREDS: usize = 32;
    let shreds = vec![Shred::new_empty_data_shred(); NUM_SHREDS];
    let mut stakes = HashMap::new();
    const NUM_PEERS: usize = 200;
    for _ in 0..NUM_PEERS {
        let id = pubkey::new_rand();
        let contact_info = ContactInfo::new_localhost(&id, timestamp());
        cluster_info.insert_info(contact_info);
        stakes.insert(id, thread_rng().gen_range(1, NUM_PEERS) as u64);
    }
    let cluster_info = Arc::new(cluster_info);
    let cluster_nodes = ClusterNodes::<BroadcastStage>::new(&cluster_info, &stakes);
    let shreds = Arc::new(shreds);
    let last_datapoint = Arc::new(AtomicU64::new(0));
    bencher.iter(move || {
        let shreds = shreds.clone();
        broadcast_shreds(
            &socket,
            &shreds,
            &cluster_nodes,
            &last_datapoint,
            &mut TransmitShredsStats::default(),
        )
        .unwrap();
    });
}

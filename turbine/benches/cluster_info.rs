#![feature(test)]

extern crate test;

use {
    rand::{thread_rng, Rng},
    solana_gossip::{
        cluster_info::{ClusterInfo, Node},
        contact_info::ContactInfo,
    },
    solana_ledger::{
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        shred::{Shred, ShredFlags},
    },
    solana_runtime::{bank::Bank, bank_forks::BankForks},
    solana_sdk::{
        pubkey,
        signature::{Keypair, Signer},
        timing::{timestamp, AtomicInterval},
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_turbine::{
        broadcast_stage::{
            broadcast_metrics::TransmitShredsStats, broadcast_shreds, BroadcastStage,
        },
        cluster_nodes::ClusterNodesCache,
    },
    std::{collections::HashMap, net::UdpSocket, sync::Arc, time::Duration},
    test::Bencher,
};

#[bench]
fn broadcast_shreds_bench(bencher: &mut Bencher) {
    solana_logger::setup();
    let leader_keypair = Arc::new(Keypair::new());
    let (quic_endpoint_sender, _quic_endpoint_receiver) =
        tokio::sync::mpsc::channel(/*capacity:*/ 128);
    let leader_info = Node::new_localhost_with_pubkey(&leader_keypair.pubkey());
    let cluster_info = ClusterInfo::new(
        leader_info.info,
        leader_keypair,
        SocketAddrSpace::Unspecified,
    );
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
    let bank = Bank::new_for_benches(&genesis_config);
    let bank_forks = BankForks::new_rw_arc(bank);

    const NUM_SHREDS: usize = 32;
    let shred = Shred::new_from_data(0, 0, 0, &[], ShredFlags::empty(), 0, 0, 0);
    let shreds = vec![shred; NUM_SHREDS];
    let mut stakes = HashMap::new();
    const NUM_PEERS: usize = 200;
    for _ in 0..NUM_PEERS {
        let id = pubkey::new_rand();
        let contact_info = ContactInfo::new_localhost(&id, timestamp());
        cluster_info.insert_info(contact_info);
        stakes.insert(id, thread_rng().gen_range(1..NUM_PEERS) as u64);
    }
    let cluster_info = Arc::new(cluster_info);
    let cluster_nodes_cache = ClusterNodesCache::<BroadcastStage>::new(
        8,                      // cap
        Duration::from_secs(5), // ttl
    );
    let shreds = Arc::new(shreds);
    let last_datapoint = Arc::new(AtomicInterval::default());
    bencher.iter(move || {
        let shreds = shreds.clone();
        broadcast_shreds(
            &socket,
            &shreds,
            &cluster_nodes_cache,
            &last_datapoint,
            &mut TransmitShredsStats::default(),
            &cluster_info,
            &bank_forks,
            &SocketAddrSpace::Unspecified,
            &quic_endpoint_sender,
        )
        .unwrap();
    });
}

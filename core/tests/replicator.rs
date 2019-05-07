#[macro_use]
extern crate log;

#[macro_use]
extern crate solana;

use bincode::{deserialize, serialize};
use solana::blocktree::{create_new_tmp_ledger, Blocktree};
use solana::cluster_info::{ClusterInfo, Node, FULLNODE_PORT_RANGE};
use solana::contact_info::ContactInfo;
use solana::fullnode::FullnodeConfig;
use solana::gossip_service::discover_nodes;
use solana::local_cluster::{ClusterConfig, LocalCluster};
use solana::replicator::Replicator;
use solana::replicator::ReplicatorRequest;
use solana::storage_stage::STORAGE_ROTATE_TEST_COUNT;
use solana::streamer::blob_receiver;
use solana_client::thin_client::create_client;
use solana_sdk::genesis_block::create_genesis_block;
use solana_sdk::hash::Hash;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::fs::remove_dir_all;
use std::net::SocketAddr;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

fn get_slot_height(to: SocketAddr) -> u64 {
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let req = ReplicatorRequest::GetSlotHeight(socket.local_addr().unwrap());
    let serialized_req = serialize(&req).unwrap();
    for _ in 0..10 {
        socket.send_to(&serialized_req, to).unwrap();
        let mut buf = [0; 1024];
        if let Ok((size, _addr)) = socket.recv_from(&mut buf) {
            return deserialize(&buf[..size]).unwrap();
        }
        sleep(Duration::from_millis(500));
    }
    panic!("Couldn't get slot height!");
}

fn download_from_replicator(replicator_info: &ContactInfo) {
    // Create a client which downloads from the replicator and see that it
    // can respond with blobs.
    let tn = Node::new_localhost();
    let cluster_info = ClusterInfo::new_with_invalid_keypair(tn.info.clone());
    let mut repair_index = get_slot_height(replicator_info.storage_addr);
    info!("repair index: {}", repair_index);

    repair_index = 0;
    let req = cluster_info
        .window_index_request_bytes(0, repair_index)
        .unwrap();

    let exit = Arc::new(AtomicBool::new(false));
    let (s_reader, r_reader) = channel();
    let repair_socket = Arc::new(tn.sockets.repair);
    let t_receiver = blob_receiver(repair_socket.clone(), &exit, s_reader);

    info!(
        "Sending repair requests from: {} to: {}",
        tn.info.id, replicator_info.gossip
    );

    let mut received_blob = false;
    for _ in 0..5 {
        repair_socket.send_to(&req, replicator_info.gossip).unwrap();

        let x = r_reader.recv_timeout(Duration::new(1, 0));

        if let Ok(blobs) = x {
            for b in blobs {
                let br = b.read().unwrap();
                assert!(br.index() == repair_index);
                info!("br: {:?}", br);
                let entries = Blocktree::deserialize_blob_data(&br.data()).unwrap();
                for entry in &entries {
                    info!("entry: {:?}", entry);
                    assert_ne!(entry.hash, Hash::default());
                    received_blob = true;
                }
            }
            break;
        }
    }
    exit.store(true, Ordering::Relaxed);
    t_receiver.join().unwrap();

    assert!(received_blob);
}

/// Start the cluster with the given configuration and wait till the replicators are discovered
/// Then download blobs from one of them.
fn run_replicator_startup_basic(num_nodes: usize, num_replicators: usize) {
    solana_logger::setup();
    info!("starting replicator test");

    let mut fullnode_config = FullnodeConfig::default();
    fullnode_config.storage_rotate_count = STORAGE_ROTATE_TEST_COUNT;
    let config = ClusterConfig {
        fullnode_config,
        num_replicators: num_replicators as u64,
        node_stakes: vec![100; num_nodes],
        cluster_lamports: 10_000,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&config);

    let cluster_nodes = discover_nodes(
        &cluster.entry_point_info.gossip,
        num_nodes + num_replicators,
    )
    .unwrap();
    assert_eq!(cluster_nodes.len(), num_nodes + num_replicators);
    let mut replicator_count = 0;
    let mut replicator_info = ContactInfo::default();
    for node in &cluster_nodes {
        info!("storage: {:?} rpc: {:?}", node.storage_addr, node.rpc);
        if ContactInfo::is_valid_address(&node.storage_addr) {
            replicator_count += 1;
            replicator_info = node.clone();
        }
    }
    assert_eq!(replicator_count, num_replicators);

    download_from_replicator(&replicator_info);
}

#[test]
fn test_replicator_startup_1_node() {
    run_replicator_startup_basic(1, 1);
}

#[test]
fn test_replicator_startup_2_nodes() {
    run_replicator_startup_basic(2, 1);
}

#[test]
fn test_replicator_startup_leader_hang() {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::Duration;

    solana_logger::setup();
    info!("starting replicator test");

    let leader_ledger_path = "replicator_test_leader_ledger";
    let (genesis_block, _mint_keypair) = create_genesis_block(10_000);
    let (replicator_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);

    {
        let replicator_keypair = Arc::new(Keypair::new());
        let storage_keypair = Arc::new(Keypair::new());

        info!("starting replicator node");
        let replicator_node = Node::new_localhost_with_pubkey(&replicator_keypair.pubkey());

        let fake_gossip = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let leader_info = ContactInfo::new_gossip_entry_point(&fake_gossip);

        let replicator_res = Replicator::new(
            &replicator_ledger_path,
            replicator_node,
            leader_info,
            replicator_keypair,
            storage_keypair,
            Some(Duration::from_secs(3)),
        );

        assert!(replicator_res.is_err());
    }

    let _ignored = Blocktree::destroy(&leader_ledger_path);
    let _ignored = Blocktree::destroy(&replicator_ledger_path);
    let _ignored = remove_dir_all(&leader_ledger_path);
    let _ignored = remove_dir_all(&replicator_ledger_path);
}

#[test]
fn test_replicator_startup_ledger_hang() {
    solana_logger::setup();
    info!("starting replicator test");
    let mut fullnode_config = FullnodeConfig::default();
    fullnode_config.storage_rotate_count = STORAGE_ROTATE_TEST_COUNT;
    let cluster = LocalCluster::new_with_equal_stakes(2, 10_000, 100);;

    info!("starting replicator node");
    let bad_keys = Arc::new(Keypair::new());
    let storage_keypair = Arc::new(Keypair::new());
    let mut replicator_node = Node::new_localhost_with_pubkey(&bad_keys.pubkey());

    // Pass bad TVU sockets to prevent successful ledger download
    replicator_node.sockets.tvu = vec![std::net::UdpSocket::bind("0.0.0.0:0").unwrap()];
    let (replicator_ledger_path, _blockhash) = create_new_tmp_ledger!(&cluster.genesis_block);

    let replicator_res = Replicator::new(
        &replicator_ledger_path,
        replicator_node,
        cluster.entry_point_info.clone(),
        bad_keys,
        storage_keypair,
        Some(Duration::from_secs(3)),
    );

    assert!(replicator_res.is_err());
}

#[test]
fn test_account_setup() {
    let num_nodes = 1;
    let num_replicators = 1;
    let mut fullnode_config = FullnodeConfig::default();
    fullnode_config.storage_rotate_count = STORAGE_ROTATE_TEST_COUNT;
    let config = ClusterConfig {
        fullnode_config,
        num_replicators,
        node_stakes: vec![100; num_nodes],
        cluster_lamports: 10_000,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&config);

    let _ = discover_nodes(
        &cluster.entry_point_info.gossip,
        num_nodes + num_replicators as usize,
    )
    .unwrap();
    // now check that the cluster actually has accounts for the replicator.
    let client = create_client(
        cluster.entry_point_info.client_facing_addr(),
        FULLNODE_PORT_RANGE,
    );
    cluster.replicator_infos.iter().for_each(|(_, value)| {
        assert_eq!(
            client
                .poll_get_balance(&value.replicator_storage_id)
                .unwrap(),
            1
        );
    });
}

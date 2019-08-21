#[macro_use]
extern crate log;

#[macro_use]
extern crate solana_core;

use serial_test_derive::serial;
use solana_client::thin_client::create_client;
use solana_core::blocktree::{create_new_tmp_ledger, get_tmp_ledger_path, Blocktree};
use solana_core::cluster_info::{ClusterInfo, Node, FULLNODE_PORT_RANGE};
use solana_core::contact_info::ContactInfo;
use solana_core::gossip_service::discover_cluster;
use solana_core::replicator::Replicator;
use solana_core::storage_stage::SLOTS_PER_TURN_TEST;
use solana_core::validator::ValidatorConfig;
use solana_local_cluster::local_cluster::{ClusterConfig, LocalCluster};
use solana_sdk::genesis_block::create_genesis_block;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::fs::remove_dir_all;
use std::sync::{Arc, RwLock};

/// Start the cluster with the given configuration and wait till the replicators are discovered
/// Then download blobs from one of them.
fn run_replicator_startup_basic(num_nodes: usize, num_replicators: usize) {
    solana_logger::setup();
    info!("starting replicator test");

    let mut validator_config = ValidatorConfig::default();
    let slots_per_segment = 8;
    validator_config.storage_slots_per_turn = SLOTS_PER_TURN_TEST;
    let config = ClusterConfig {
        validator_configs: vec![validator_config; num_nodes],
        num_replicators,
        node_stakes: vec![100; num_nodes],
        cluster_lamports: 10_000,
        // keep a low slot/segment count to speed up the test
        slots_per_segment,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&config);

    let (cluster_nodes, cluster_replicators) = discover_cluster(
        &cluster.entry_point_info.gossip,
        num_nodes + num_replicators,
    )
    .unwrap();
    assert_eq!(
        cluster_nodes.len() + cluster_replicators.len(),
        num_nodes + num_replicators
    );
    let mut replicator_count = 0;
    let mut replicator_info = ContactInfo::default();
    for node in &cluster_replicators {
        info!("storage: {:?} rpc: {:?}", node.storage_addr, node.rpc);
        if ContactInfo::is_valid_address(&node.storage_addr) {
            replicator_count += 1;
            replicator_info = node.clone();
        }
    }
    assert_eq!(replicator_count, num_replicators);

    let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
        cluster_nodes[0].clone(),
    )));
    let path = get_tmp_ledger_path("test");
    let blocktree = Arc::new(Blocktree::open(&path).unwrap());
    Replicator::download_from_replicator(
        &cluster_info,
        &replicator_info,
        &blocktree,
        slots_per_segment,
    )
    .unwrap();
}

#[test]
#[ignore]
fn test_replicator_startup_1_node() {
    run_replicator_startup_basic(1, 1);
}

#[test]
#[ignore]
fn test_replicator_startup_2_nodes() {
    run_replicator_startup_basic(2, 1);
}

#[test]
#[serial]
fn test_replicator_startup_leader_hang() {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    solana_logger::setup();
    info!("starting replicator test");

    let leader_ledger_path = std::path::PathBuf::from("replicator_test_leader_ledger");
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
        );

        assert!(replicator_res.is_err());
    }

    let _ignored = Blocktree::destroy(&leader_ledger_path);
    let _ignored = Blocktree::destroy(&replicator_ledger_path);
    let _ignored = remove_dir_all(&leader_ledger_path);
    let _ignored = remove_dir_all(&replicator_ledger_path);
}

#[test]
#[serial]
fn test_replicator_startup_ledger_hang() {
    solana_logger::setup();
    info!("starting replicator test");
    let mut validator_config = ValidatorConfig::default();
    validator_config.storage_slots_per_turn = SLOTS_PER_TURN_TEST;
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
    );

    assert!(replicator_res.is_err());
}

#[test]
#[serial]
fn test_account_setup() {
    let num_nodes = 1;
    let num_replicators = 1;
    let mut validator_config = ValidatorConfig::default();
    validator_config.storage_slots_per_turn = SLOTS_PER_TURN_TEST;
    let config = ClusterConfig {
        validator_configs: vec![ValidatorConfig::default(); num_nodes],
        num_replicators,
        node_stakes: vec![100; num_nodes],
        cluster_lamports: 10_000,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&config);

    let _ = discover_cluster(
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
                .poll_get_balance(&value.replicator_storage_pubkey)
                .unwrap(),
            1
        );
    });
}

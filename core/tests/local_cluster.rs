extern crate solana;

use solana::cluster::Cluster;
use solana::cluster_tests;
use solana::fullnode::FullnodeConfig;
use solana::gossip_service::discover_cluster;
use solana::local_cluster::{ClusterConfig, LocalCluster};
use solana::poh_service::PohServiceConfig;
use solana_runtime::epoch_schedule::MINIMUM_SLOT_LENGTH;
use solana_sdk::timing;
use std::time::Duration;

#[test]
fn test_spend_and_verify_all_nodes_1() {
    solana_logger::setup();
    let num_nodes = 1;
    let local = LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
    );
}

#[test]
fn test_spend_and_verify_all_nodes_2() {
    solana_logger::setup();
    let num_nodes = 2;
    let local = LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
    );
}

#[test]
fn test_spend_and_verify_all_nodes_3() {
    solana_logger::setup();
    let num_nodes = 3;
    let local = LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
    );
}

#[test]
#[ignore]
fn test_spend_and_verify_all_nodes_env_num_nodes() {
    solana_logger::setup();
    let num_nodes: usize = std::env::var("NUM_NODES")
        .expect("please set environment variable NUM_NODES")
        .parse()
        .expect("could not parse NUM_NODES as a number");
    let local = LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
    );
}

#[test]
#[should_panic]
fn test_fullnode_exit_default_config_should_panic() {
    solana_logger::setup();
    let num_nodes = 2;
    let local = LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100);
    cluster_tests::fullnode_exit(&local.entry_point_info, num_nodes);
}

#[test]
fn test_fullnode_exit_2() {
    solana_logger::setup();
    let num_nodes = 2;
    let mut fullnode_config = FullnodeConfig::default();
    fullnode_config.rpc_config.enable_fullnode_exit = true;
    let config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100; 2],
        fullnode_config,
        ..ClusterConfig::default()
    };
    let local = LocalCluster::new(&config);
    cluster_tests::fullnode_exit(&local.entry_point_info, num_nodes);
}

// Cluster needs a supermajority to remain, so the minimum size for this test is 4
#[test]
fn test_leader_failure_4() {
    solana_logger::setup();
    let num_nodes = 4;
    let mut fullnode_config = FullnodeConfig::default();
    fullnode_config.rpc_config.enable_fullnode_exit = true;
    let config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100; 4],
        fullnode_config: fullnode_config.clone(),
        ..ClusterConfig::default()
    };
    let local = LocalCluster::new(&config);
    cluster_tests::kill_entry_and_spend_and_verify_rest(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
        fullnode_config
            .tick_config
            .ticks_to_ms(config.ticks_per_slot as u64),
    );
}
#[test]
fn test_two_unbalanced_stakes() {
    solana_logger::setup();
    let mut fullnode_config = FullnodeConfig::default();
    let num_ticks_per_second = 100;
    let num_ticks_per_slot = 40;
    let num_slots_per_epoch = MINIMUM_SLOT_LENGTH as u64;
    fullnode_config.tick_config =
        PohServiceConfig::Sleep(Duration::from_millis(1000 / num_ticks_per_second));
    fullnode_config.rpc_config.enable_fullnode_exit = true;
    let mut cluster = LocalCluster::new(&ClusterConfig {
        node_stakes: vec![999_990, 3],
        cluster_lamports: 1_000_000,
        fullnode_config: fullnode_config.clone(),
        ticks_per_slot: num_ticks_per_slot,
        slots_per_epoch: num_slots_per_epoch,
        ..ClusterConfig::default()
    });

    cluster_tests::sleep_n_epochs(
        10.0,
        &fullnode_config.tick_config,
        num_ticks_per_slot,
        num_slots_per_epoch,
    );
    cluster.close_preserve_ledgers();
    let leader_id = cluster.entry_point_info.id;
    let leader_ledger = cluster.fullnode_infos[&leader_id].ledger_path.clone();
    cluster_tests::verify_ledger_ticks(&leader_ledger, num_ticks_per_slot as usize);
}

#[test]
#[ignore]
fn test_forwarding() {
    // Set up a cluster where one node is never the leader, so all txs sent to this node
    // will be have to be forwarded in order to be confirmed
    let config = ClusterConfig {
        node_stakes: vec![999_990, 3],
        cluster_lamports: 2_000_000,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&config);

    let (cluster_nodes, _) = discover_cluster(&cluster.entry_point_info.gossip, 2).unwrap();
    assert!(cluster_nodes.len() >= 2);

    let leader_id = cluster.entry_point_info.id;

    let validator_info = cluster_nodes.iter().find(|c| c.id != leader_id).unwrap();

    // Confirm that transactions were forwarded to and processed by the leader.
    cluster_tests::send_many_transactions(&validator_info, &cluster.funding_keypair, 20);
}

#[test]
fn test_restart_node() {
    let fullnode_config = FullnodeConfig::default();
    let slots_per_epoch = MINIMUM_SLOT_LENGTH as u64;
    let ticks_per_slot = 16;
    let mut cluster = LocalCluster::new(&ClusterConfig {
        node_stakes: vec![3],
        cluster_lamports: 100,
        fullnode_config: fullnode_config.clone(),
        ticks_per_slot,
        slots_per_epoch,
        ..ClusterConfig::default()
    });
    let nodes = cluster.get_node_ids();
    cluster_tests::sleep_n_epochs(
        1.0,
        &fullnode_config.tick_config,
        timing::DEFAULT_TICKS_PER_SLOT,
        slots_per_epoch,
    );
    cluster.restart_node(nodes[0]);
    cluster_tests::sleep_n_epochs(
        0.5,
        &fullnode_config.tick_config,
        timing::DEFAULT_TICKS_PER_SLOT,
        slots_per_epoch,
    );
    cluster_tests::send_many_transactions(&cluster.entry_point_info, &cluster.funding_keypair, 1);
}

#[test]
fn test_listener_startup() {
    let config = ClusterConfig {
        node_stakes: vec![100; 1],
        cluster_lamports: 1_000,
        num_listeners: 3,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&config);
    let (cluster_nodes, _) = discover_cluster(&cluster.entry_point_info.gossip, 4).unwrap();
    assert_eq!(cluster_nodes.len(), 4);
}

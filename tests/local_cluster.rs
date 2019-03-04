extern crate solana;

use solana::cluster_tests;
use solana::fullnode::FullnodeConfig;
use solana::local_cluster::LocalCluster;
use solana::rpc::JsonRpcConfig;
use std::time::Duration;

#[test]
fn test_spend_and_verify_all_nodes_1() -> () {
    solana_logger::setup();
    let num_nodes = 1;
    let local = LocalCluster::new(num_nodes, 10_000, 100);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
    );
}

#[test]
fn test_spend_and_verify_all_nodes_2() -> () {
    solana_logger::setup();
    let num_nodes = 2;
    let local = LocalCluster::new(num_nodes, 10_000, 100);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
    );
}

#[test]
fn test_spend_and_verify_all_nodes_3() -> () {
    solana_logger::setup();
    let num_nodes = 3;
    let local = LocalCluster::new(num_nodes, 10_000, 100);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
    );
}

#[test]
#[should_panic]
fn test_fullnode_exit_safe_config_should_panic_2() -> () {
    solana_logger::setup();
    let num_nodes = 2;
    let local = LocalCluster::new(num_nodes, 10_000, 100);
    cluster_tests::fullnode_exit(&local.entry_point_info, num_nodes);
}

#[test]
fn test_fullnode_exit_unsafe_config_2() -> () {
    solana_logger::setup();
    let num_nodes = 2;
    let mut fullnode_exit = FullnodeConfig::default();
    fullnode_exit.rpc_config = JsonRpcConfig::TestOnlyAllowRpcFullnodeExit;
    let local = LocalCluster::new_with_config(num_nodes, 10_000, 100, &fullnode_exit);
    cluster_tests::fullnode_exit(&local.entry_point_info, num_nodes);
}

#[test]
#[ignore]
fn test_rotate_nodes_3() -> () {
    solana_logger::setup();
    let num_nodes = 3;
    let local = LocalCluster::new(num_nodes, 10_000, 100);
    cluster_tests::rotate_leader_through_all_nodes(
        &local.entry_point_info,
        num_nodes,
        Duration::from_secs(num_nodes as u64 * 60),
    );

    // Test that transactions can go through
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
    );
}

#[test]
#[ignore]
fn test_rotate_nodes_remove_leader_3() -> () {
    solana_logger::setup();
    let num_nodes = 3;
    let mut fullnode_exit = FullnodeConfig::default();
    fullnode_exit.rpc_config = JsonRpcConfig::TestOnlyAllowRpcFullnodeExit;
    let local = LocalCluster::new_with_config(num_nodes, 10_000, 100, &fullnode_exit);

    // Check that all nodes cycle through leader role
    cluster_tests::rotate_leader_through_all_nodes(
        &local.entry_point_info,
        num_nodes,
        Duration::from_secs(num_nodes as u64 * 60),
    );

    // Exit the leader node
    cluster_tests::fullnode_exit(&local.entry_point_info, num_nodes);
}

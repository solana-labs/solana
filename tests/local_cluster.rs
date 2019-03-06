extern crate solana;

use solana::cluster_tests;
use solana::fullnode::FullnodeConfig;
use solana::local_cluster::LocalCluster;

#[test]
fn test_spend_and_verify_all_nodes_1() {
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
fn test_spend_and_verify_all_nodes_2() {
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
fn test_spend_and_verify_all_nodes_3() {
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
fn test_fullnode_exit_default_config_should_panic() {
    solana_logger::setup();
    let num_nodes = 2;
    let local = LocalCluster::new(num_nodes, 10_000, 100);
    cluster_tests::fullnode_exit(&local.entry_point_info, num_nodes);
}

#[test]
fn test_fullnode_exit_2() {
    solana_logger::setup();
    let num_nodes = 2;
    let mut fullnode_config = FullnodeConfig::default();
    fullnode_config.rpc_config.enable_fullnode_exit = true;
    let local = LocalCluster::new_with_config(num_nodes, 10_000, 100, &fullnode_config);
    cluster_tests::fullnode_exit(&local.entry_point_info, num_nodes);
}

#[test]
fn test_leader_failure_2() {
    let num_nodes = 2;
    let mut fullnode_config = FullnodeConfig::default();
    fullnode_config.rpc_config.enable_fullnode_exit = true;
    let local = LocalCluster::new_with_config(num_nodes, 10_000, 100, &fullnode_config);
    cluster_tests::kill_entry_and_spend_and_verify_rest(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
    );
}

#[test]
fn test_leader_failure_3() {
    let num_nodes = 3;
    let mut fullnode_config = FullnodeConfig::default();
    fullnode_config.rpc_config.enable_fullnode_exit = true;
    let local = LocalCluster::new_with_config(num_nodes, 10_000, 100, &fullnode_config);
    cluster_tests::kill_entry_and_spend_and_verify_rest(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
    );
}

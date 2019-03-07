extern crate solana;

use solana::cluster_tests;
use solana::fullnode::FullnodeConfig;
use solana::local_cluster::LocalCluster;
use solana::poh_service::PohServiceConfig;
use solana_sdk::timing::{DEFAULT_SLOTS_PER_EPOCH, DEFAULT_TICKS_PER_SLOT};
use std::thread::sleep;
use std::time::Duration;

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
    let local = LocalCluster::new_with_config(&[100; 2], 10_000, &fullnode_config);
    cluster_tests::fullnode_exit(&local.entry_point_info, num_nodes);
}

#[test]
fn test_leader_failure_2() {
    let num_nodes = 2;
    let mut fullnode_config = FullnodeConfig::default();
    fullnode_config.rpc_config.enable_fullnode_exit = true;
    let local = LocalCluster::new_with_config(&[100; 2], 10_000, &fullnode_config);
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
    let local = LocalCluster::new_with_config(&[100; 3], 10_000, &fullnode_config);
    cluster_tests::kill_entry_and_spend_and_verify_rest(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
    );
}

#[test]
fn test_two_unbalanced_stakes() {
    let mut fullnode_config = FullnodeConfig::default();
    let num_ticks_per_second = 100;
    fullnode_config.tick_config =
        PohServiceConfig::Sleep(Duration::from_millis(100 / num_ticks_per_second));
    fullnode_config.rpc_config.enable_fullnode_exit = true;
    let mut cluster = LocalCluster::new_with_config(&[999_990, 3], 1_000_000, &fullnode_config);
    let num_epochs_to_sleep = 10;
    let num_ticks_to_sleep = num_epochs_to_sleep * DEFAULT_TICKS_PER_SLOT * DEFAULT_SLOTS_PER_EPOCH;
    sleep(Duration::from_millis(
        num_ticks_to_sleep / num_ticks_per_second * 100,
    ));

    cluster.close_preserve_ledgers();
    let leader_ledger = cluster.ledger_paths[1].clone();
    cluster_tests::verify_ledger_ticks(&leader_ledger, DEFAULT_TICKS_PER_SLOT as usize);

    drop(cluster);
}

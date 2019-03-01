extern crate solana;

use solana::cluster_tests;
use solana::local_cluster::LocalCluster;

#[test]
fn test_spend_and_verify_all_nodes_1() -> () {
    solana_logger::setup();
    let num_nodes = 1;
    let local = LocalCluster::create_network(num_nodes, 10_000, 100);
    cluster_tests::spend_and_verify_all_nodes(&local.contact_info, &local.mint, num_nodes);
}

#[test]
fn test_spend_and_verify_all_nodes_2() -> () {
    solana_logger::setup();
    let num_nodes = 2;
    let local = LocalCluster::create_network(num_nodes, 10_000, 100);
    cluster_tests::spend_and_verify_all_nodes(&local.contact_info, &local.mint, num_nodes);
}

#[test]
fn test_spend_and_verify_all_nodes_3() -> () {
    solana_logger::setup();
    let num_nodes = 3;
    let local = LocalCluster::create_network(num_nodes, 10_000, 100);
    cluster_tests::spend_and_verify_all_nodes(&local.contact_info, &local.mint, num_nodes);
}

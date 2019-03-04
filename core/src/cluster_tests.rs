/// Cluster independent integration tests
///
/// All tests must start from an entry point and a funding keypair and
/// discover the rest of the network.
use crate::client::mk_client;
use crate::contact_info::ContactInfo;
use crate::gossip_service::discover;
use crate::thin_client;
use hashbrown::HashMap;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

/// Spend and verify from every node in the network
pub fn spend_and_verify_all_nodes(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    nodes: usize,
) {
    let cluster_nodes = discover(&entry_point_info, nodes);
    assert!(cluster_nodes.len() >= nodes);
    for ingress_node in &cluster_nodes {
        let random_keypair = Keypair::new();
        let mut client = mk_client(&ingress_node);
        let bal = client
            .poll_get_balance(&funding_keypair.pubkey())
            .expect("balance in source");
        assert!(bal > 0);
        let mut transaction = SystemTransaction::new_move(
            &funding_keypair,
            random_keypair.pubkey(),
            1,
            client.get_recent_blockhash(),
            0,
        );
        let sig = client
            .retry_transfer(&funding_keypair, &mut transaction, 5)
            .unwrap();
        for validator in &cluster_nodes {
            let mut client = mk_client(&validator);
            client.poll_for_signature(&sig).unwrap();
        }
    }
}

pub fn fullnode_exit(entry_point_info: &ContactInfo, nodes: usize) {
    let cluster_nodes = discover(&entry_point_info, nodes);
    assert!(cluster_nodes.len() >= nodes);
    for node in &cluster_nodes {
        let mut client = mk_client(&node);
        assert!(client.fullnode_exit().unwrap());
    }
    sleep(Duration::from_millis(250));
    for node in &cluster_nodes {
        let mut client = mk_client(&node);
        assert!(client.fullnode_exit().is_err());
    }
}

pub fn wait_for_abs_num_nodes(entry_point_info: &ContactInfo, nodes: usize, timeout: Duration) {
    let mut cluster_nodes = discover(&entry_point_info, nodes).len();

    let now = Instant::now();
    while cluster_nodes != nodes {
        assert!(now.elapsed() < timeout);
        cluster_nodes = discover(&entry_point_info, nodes).len();
    }
}

pub fn rotate_leader_through_all_nodes(
    entry_point_info: &ContactInfo,
    nodes: usize,
    timeout: Duration,
) {
    let cluster_nodes = discover(&entry_point_info, nodes);
    let mut node_was_leader = HashMap::new();

    let now = Instant::now();
    while node_was_leader.len() < nodes {
        assert!(now.elapsed() < timeout);
        let leader = thin_client::poll_gossip_for_leader(cluster_nodes[0].gossip, Some(5)).unwrap();
        node_was_leader.insert(leader.id, true);
    }
}

/// Cluster independant integration tests
///
/// All tests must start from an entry point and a funding keypair and
/// discover the rest of the network.
use crate::client::mk_client;
use crate::contact_info::ContactInfo;
use crate::gossip_service::discover;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::timing::{DEFAULT_SLOTS_PER_EPOCH, DEFAULT_TICKS_PER_SLOT, NUM_TICKS_PER_SECOND};
use std::thread::sleep;
use std::time::Duration;

const SLOT_MILLIS: u64 = (DEFAULT_TICKS_PER_SLOT * 1000) / NUM_TICKS_PER_SECOND;

/// Spend and verify from every node in the network
pub fn spend_and_verify_all_nodes(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    nodes: usize,
) {
    let cluster_nodes = discover(&entry_point_info, nodes).unwrap();
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
    let cluster_nodes = discover(&entry_point_info, nodes).unwrap();
    assert!(cluster_nodes.len() >= nodes);
    for node in &cluster_nodes {
        let mut client = mk_client(&node);
        assert!(client.fullnode_exit().unwrap());
    }
    sleep(Duration::from_millis(SLOT_MILLIS));
    for node in &cluster_nodes {
        let mut client = mk_client(&node);
        assert!(client.fullnode_exit().is_err());
    }
}

pub fn kill_entry_and_spend_and_verify_rest(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    nodes: usize,
) {
    solana_logger::setup();
    let cluster_nodes = discover(&entry_point_info, nodes).unwrap();
    assert!(cluster_nodes.len() >= nodes);
    let mut client = mk_client(&entry_point_info);
    info!("sleeping for an epoch");
    sleep(Duration::from_millis(SLOT_MILLIS * DEFAULT_SLOTS_PER_EPOCH));
    info!("done sleeping for an epoch");
    info!("killing entry point");
    assert!(client.fullnode_exit().unwrap());
    info!("sleeping for a slot");
    sleep(Duration::from_millis(SLOT_MILLIS));
    info!("done sleeping for a slot");
    for ingress_node in &cluster_nodes {
        if ingress_node.id == entry_point_info.id {
            continue;
        }
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
            if validator.id == entry_point_info.id {
                continue;
            }
            let mut client = mk_client(&validator);
            client.poll_for_signature(&sig).unwrap();
        }
    }
}

/// Cluster independant integration tests
///
/// All tests must start from an entry point and a funding keypair and
/// discover the rest of the network.
use solana::{
    blocktree::Blocktree,
    cluster_info::FULLNODE_PORT_RANGE,
    consensus::VOTE_THRESHOLD_DEPTH,
    contact_info::ContactInfo,
    entry::{Entry, EntrySlice},
    gossip_service::discover_cluster,
};
use solana_client::thin_client::create_client;
use solana_runtime::epoch_schedule::MINIMUM_SLOTS_PER_EPOCH;
use solana_sdk::{
    client::SyncClient,
    hash::Hash,
    poh_config::PohConfig,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil, Signature},
    system_transaction,
    timing::{
        duration_as_ms, DEFAULT_NUM_TICKS_PER_SECOND, DEFAULT_TICKS_PER_SLOT,
        NUM_CONSECUTIVE_LEADER_SLOTS,
    },
    transport::TransportError,
};
use std::{collections::HashSet, path::Path, thread::sleep, time::Duration};

const DEFAULT_SLOT_MILLIS: u64 = (DEFAULT_TICKS_PER_SLOT * 1000) / DEFAULT_NUM_TICKS_PER_SECOND;

/// Spend and verify from every node in the network
pub fn spend_and_verify_all_nodes<S: ::std::hash::BuildHasher>(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    nodes: usize,
    ignore_nodes: HashSet<Pubkey, S>,
) {
    let (cluster_nodes, _) = discover_cluster(&entry_point_info.gossip, nodes).unwrap();
    assert!(cluster_nodes.len() >= nodes);
    for ingress_node in &cluster_nodes {
        if ignore_nodes.contains(&ingress_node.id) {
            continue;
        }
        let random_keypair = Keypair::new();
        let client = create_client(ingress_node.client_facing_addr(), FULLNODE_PORT_RANGE);
        let bal = client
            .poll_get_balance(&funding_keypair.pubkey())
            .expect("balance in source");
        assert!(bal > 0);
        let (blockhash, _fee_calculator) = client.get_recent_blockhash().unwrap();
        let mut transaction =
            system_transaction::transfer(&funding_keypair, &random_keypair.pubkey(), 1, blockhash);
        let confs = VOTE_THRESHOLD_DEPTH + 1;
        let sig = client
            .retry_transfer_until_confirmed(&funding_keypair, &mut transaction, 5, confs)
            .unwrap();
        for validator in &cluster_nodes {
            if ignore_nodes.contains(&validator.id) {
                continue;
            }
            let client = create_client(validator.client_facing_addr(), FULLNODE_PORT_RANGE);
            client.poll_for_signature_confirmation(&sig, confs).unwrap();
        }
    }
}

pub fn send_many_transactions(node: &ContactInfo, funding_keypair: &Keypair, num_txs: u64) {
    let client = create_client(node.client_facing_addr(), FULLNODE_PORT_RANGE);
    for _ in 0..num_txs {
        let random_keypair = Keypair::new();
        let bal = client
            .poll_get_balance(&funding_keypair.pubkey())
            .expect("balance in source");
        assert!(bal > 0);
        let (blockhash, _fee_calculator) = client.get_recent_blockhash().unwrap();
        let mut transaction =
            system_transaction::transfer(&funding_keypair, &random_keypair.pubkey(), 1, blockhash);
        client
            .retry_transfer(&funding_keypair, &mut transaction, 5)
            .unwrap();
    }
}

pub fn fullnode_exit(entry_point_info: &ContactInfo, nodes: usize) {
    let (cluster_nodes, _) = discover_cluster(&entry_point_info.gossip, nodes).unwrap();
    assert!(cluster_nodes.len() >= nodes);
    for node in &cluster_nodes {
        let client = create_client(node.client_facing_addr(), FULLNODE_PORT_RANGE);
        assert!(client.fullnode_exit().unwrap());
    }
    sleep(Duration::from_millis(DEFAULT_SLOT_MILLIS));
    for node in &cluster_nodes {
        let client = create_client(node.client_facing_addr(), FULLNODE_PORT_RANGE);
        assert!(client.fullnode_exit().is_err());
    }
}

pub fn verify_ledger_ticks(ledger_path: &Path, ticks_per_slot: usize) {
    let ledger = Blocktree::open(ledger_path).unwrap();
    let zeroth_slot = ledger.get_slot_entries(0, 0, None).unwrap();
    let last_id = zeroth_slot.last().unwrap().hash;
    let next_slots = ledger.get_slots_since(&[0]).unwrap().remove(&0).unwrap();
    let mut pending_slots: Vec<_> = next_slots
        .into_iter()
        .map(|slot| (slot, 0, last_id))
        .collect();
    while !pending_slots.is_empty() {
        let (slot, parent_slot, last_id) = pending_slots.pop().unwrap();
        let next_slots = ledger
            .get_slots_since(&[slot])
            .unwrap()
            .remove(&slot)
            .unwrap();

        // If you're not the last slot, you should have a full set of ticks
        let should_verify_ticks = if !next_slots.is_empty() {
            Some((slot - parent_slot) as usize * ticks_per_slot)
        } else {
            None
        };

        let last_id = verify_slot_ticks(&ledger, slot, &last_id, should_verify_ticks);
        pending_slots.extend(
            next_slots
                .into_iter()
                .map(|child_slot| (child_slot, slot, last_id)),
        );
    }
}

pub fn sleep_n_epochs(
    num_epochs: f64,
    config: &PohConfig,
    ticks_per_slot: u64,
    slots_per_epoch: u64,
) {
    let num_ticks_per_second = (1000 / duration_as_ms(&config.target_tick_duration)) as f64;
    let num_ticks_to_sleep = num_epochs * ticks_per_slot as f64 * slots_per_epoch as f64;
    let secs = ((num_ticks_to_sleep + num_ticks_per_second - 1.0) / num_ticks_per_second) as u64;
    warn!("sleep_n_epochs: {} seconds", secs);
    sleep(Duration::from_secs(secs));
}

pub fn kill_entry_and_spend_and_verify_rest(
    entry_point_info: &ContactInfo,
    funding_keypair: &Keypair,
    nodes: usize,
    slot_millis: u64,
) {
    solana_logger::setup();
    let (cluster_nodes, _) = discover_cluster(&entry_point_info.gossip, nodes).unwrap();
    assert!(cluster_nodes.len() >= nodes);
    let client = create_client(entry_point_info.client_facing_addr(), FULLNODE_PORT_RANGE);
    let first_two_epoch_slots = MINIMUM_SLOTS_PER_EPOCH * 3;

    for ingress_node in &cluster_nodes {
        client
            .poll_get_balance(&ingress_node.id)
            .unwrap_or_else(|err| panic!("Node {} has no balance: {}", ingress_node.id, err));
    }

    info!("sleeping for 2 leader fortnights");
    sleep(Duration::from_millis(
        slot_millis * first_two_epoch_slots as u64,
    ));
    info!("done sleeping for first 2 warmup epochs");
    info!("killing entry point: {}", entry_point_info.id);
    assert!(client.fullnode_exit().unwrap());
    info!("sleeping for some time");
    sleep(Duration::from_millis(
        slot_millis * NUM_CONSECUTIVE_LEADER_SLOTS,
    ));
    info!("done sleeping for 2 fortnights");
    for ingress_node in &cluster_nodes {
        if ingress_node.id == entry_point_info.id {
            info!("ingress_node.id == entry_point_info.id, continuing...");
            continue;
        }

        let client = create_client(ingress_node.client_facing_addr(), FULLNODE_PORT_RANGE);
        let balance = client
            .poll_get_balance(&funding_keypair.pubkey())
            .expect("balance in source");
        assert_ne!(balance, 0);

        let mut result = Ok(());
        let mut retries = 0;
        loop {
            retries += 1;
            if retries > 5 {
                result.unwrap();
            }

            let random_keypair = Keypair::new();
            let (blockhash, _fee_calculator) = client.get_recent_blockhash().unwrap();
            let mut transaction = system_transaction::transfer(
                &funding_keypair,
                &random_keypair.pubkey(),
                1,
                blockhash,
            );

            let confs = VOTE_THRESHOLD_DEPTH + 1;
            let sig = {
                let sig = client.retry_transfer_until_confirmed(
                    &funding_keypair,
                    &mut transaction,
                    5,
                    confs,
                );
                match sig {
                    Err(e) => {
                        result = Err(TransportError::IoError(e));
                        continue;
                    }

                    Ok(sig) => sig,
                }
            };
            info!("poll_all_nodes_for_signature()");
            match poll_all_nodes_for_signature(&entry_point_info, &cluster_nodes, &sig, confs) {
                Err(e) => {
                    info!("poll_all_nodes_for_signature() failed {:?}", e);
                    result = Err(e);
                }
                Ok(()) => {
                    info!("poll_all_nodes_for_signature() succeeded, done.");
                    break;
                }
            }
        }
    }
}

fn poll_all_nodes_for_signature(
    entry_point_info: &ContactInfo,
    cluster_nodes: &[ContactInfo],
    sig: &Signature,
    confs: usize,
) -> Result<(), TransportError> {
    for validator in cluster_nodes {
        if validator.id == entry_point_info.id {
            continue;
        }
        let client = create_client(validator.client_facing_addr(), FULLNODE_PORT_RANGE);
        client.poll_for_signature_confirmation(&sig, confs)?;
    }

    Ok(())
}

fn get_and_verify_slot_entries(blocktree: &Blocktree, slot: u64, last_entry: &Hash) -> Vec<Entry> {
    let entries = blocktree.get_slot_entries(slot, 0, None).unwrap();
    assert!(entries.verify(last_entry));
    entries
}

fn verify_slot_ticks(
    blocktree: &Blocktree,
    slot: u64,
    last_entry: &Hash,
    expected_num_ticks: Option<usize>,
) -> Hash {
    let entries = get_and_verify_slot_entries(blocktree, slot, last_entry);
    let num_ticks: usize = entries.iter().map(|entry| entry.is_tick() as usize).sum();
    if let Some(expected_num_ticks) = expected_num_ticks {
        assert_eq!(num_ticks, expected_num_ticks);
    }
    entries.last().unwrap().hash
}

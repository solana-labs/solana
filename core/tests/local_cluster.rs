extern crate solana;

use crate::solana::blocktree::Blocktree;
use hashbrown::HashSet;
use log::*;
use solana::cluster::Cluster;
use solana::cluster_tests;
use solana::gossip_service::discover_cluster;
use solana::local_cluster::{ClusterConfig, LocalCluster};
use solana::validator::ValidatorConfig;
use solana_runtime::epoch_schedule::{EpochSchedule, MINIMUM_SLOT_LENGTH};
use solana_sdk::poh_config::PohConfig;
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
    let mut validator_config = ValidatorConfig::default();
    validator_config.rpc_config.enable_fullnode_exit = true;
    let config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100; 2],
        validator_config,
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
    let mut validator_config = ValidatorConfig::default();
    validator_config.rpc_config.enable_fullnode_exit = true;
    let config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100; 4],
        validator_config: validator_config.clone(),
        ..ClusterConfig::default()
    };
    let local = LocalCluster::new(&config);
    cluster_tests::kill_entry_and_spend_and_verify_rest(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
        config.ticks_per_slot * config.poh_config.target_tick_duration.as_millis() as u64,
    );
}
#[test]
fn test_two_unbalanced_stakes() {
    solana_logger::setup();
    let mut validator_config = ValidatorConfig::default();
    let num_ticks_per_second = 100;
    let num_ticks_per_slot = 10;
    let num_slots_per_epoch = MINIMUM_SLOT_LENGTH as u64;

    validator_config.rpc_config.enable_fullnode_exit = true;
    let mut cluster = LocalCluster::new(&ClusterConfig {
        node_stakes: vec![999_990, 3],
        cluster_lamports: 1_000_000,
        validator_config: validator_config.clone(),
        ticks_per_slot: num_ticks_per_slot,
        slots_per_epoch: num_slots_per_epoch,
        poh_config: PohConfig::new_sleep(Duration::from_millis(1000 / num_ticks_per_second)),
        ..ClusterConfig::default()
    });

    cluster_tests::sleep_n_epochs(
        10.0,
        &cluster.genesis_block.poh_config,
        num_ticks_per_slot,
        num_slots_per_epoch,
    );
    cluster.close_preserve_ledgers();
    let leader_pubkey = cluster.entry_point_info.id;
    let leader_ledger = cluster.fullnode_infos[&leader_pubkey].ledger_path.clone();
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

    let leader_pubkey = cluster.entry_point_info.id;

    let validator_info = cluster_nodes
        .iter()
        .find(|c| c.id != leader_pubkey)
        .unwrap();

    // Confirm that transactions were forwarded to and processed by the leader.
    cluster_tests::send_many_transactions(&validator_info, &cluster.funding_keypair, 20);
}

#[test]
fn test_restart_node() {
    let validator_config = ValidatorConfig::default();
    let slots_per_epoch = MINIMUM_SLOT_LENGTH as u64;
    let ticks_per_slot = 16;
    let mut cluster = LocalCluster::new(&ClusterConfig {
        node_stakes: vec![3],
        cluster_lamports: 100,
        validator_config: validator_config.clone(),
        ticks_per_slot,
        slots_per_epoch,
        ..ClusterConfig::default()
    });
    let nodes = cluster.get_node_pubkeys();
    cluster_tests::sleep_n_epochs(
        1.0,
        &cluster.genesis_block.poh_config,
        timing::DEFAULT_TICKS_PER_SLOT,
        slots_per_epoch,
    );
    cluster.restart_node(nodes[0]);
    cluster_tests::sleep_n_epochs(
        0.5,
        &cluster.genesis_block.poh_config,
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

#[test]
fn test_repairman_catchup() {
    run_repairman_catchup(5);
}

fn run_repairman_catchup(num_repairmen: u64) {
    let mut validator_config = ValidatorConfig::default();
    let num_ticks_per_second = 100;
    let num_ticks_per_slot = 40;
    let num_slots_per_epoch = MINIMUM_SLOT_LENGTH as u64;
    let num_root_buffer_slots = 10;
    // Calculate the leader schedule num_root_buffer slots ahead. Otherwise, if stakers_slot_offset ==
    // num_slots_per_epoch, and num_slots_per_epoch == MINIMUM_SLOT_LENGTH, then repairmen
    // will stop sending repairs after the last slot in epoch 1 (0-indexed), because the root
    // is at most in the first epoch.
    //
    // For example:
    // Assume:
    // 1) num_slots_per_epoch = 32
    // 2) stakers_slot_offset = 32
    // 3) MINIMUM_SLOT_LENGTH = 32
    //
    // Then the last slot in epoch 1 is slot 63. After completing slots 0 to 63, the root on the
    // repairee is at most 31. Because, the stakers_slot_offset == 32, then the max confirmed epoch
    // on the repairee is epoch 1.
    // Thus the repairmen won't send any slots past epoch 1, slot 63 to this repairee until the repairee
    // updates their root, and the repairee can't update their root until they get slot 64, so no progress
    // is made. This is also not accounting for the fact that the repairee may not vote on every slot, so
    // their root could actually be much less than 31. This is why we give a num_root_buffer_slots buffer.
    let stakers_slot_offset = num_slots_per_epoch + num_root_buffer_slots;

    validator_config.rpc_config.enable_fullnode_exit = true;

    let lamports_per_repairman = 1000;

    // Make the repairee_stake small relative to the repairmen stake so that the repairee doesn't
    // get included in the leader schedule, causing slots to get skipped while it's still trying
    // to catch up
    let repairee_stake = 3;
    let cluster_lamports = 2 * lamports_per_repairman * num_repairmen + repairee_stake;
    let node_stakes: Vec<_> = (0..num_repairmen).map(|_| lamports_per_repairman).collect();
    let mut cluster = LocalCluster::new(&ClusterConfig {
        node_stakes,
        cluster_lamports,
        validator_config: validator_config.clone(),
        ticks_per_slot: num_ticks_per_slot,
        slots_per_epoch: num_slots_per_epoch,
        stakers_slot_offset,
        poh_config: PohConfig::new_sleep(Duration::from_millis(1000 / num_ticks_per_second)),
        ..ClusterConfig::default()
    });

    let repairman_pubkeys: HashSet<_> = cluster.get_node_pubkeys().into_iter().collect();
    let epoch_schedule = EpochSchedule::new(num_slots_per_epoch, stakers_slot_offset, true);
    let num_warmup_epochs = (epoch_schedule.get_stakers_epoch(0) + 1) as f64;

    // Sleep for longer than the first N warmup epochs, with a one epoch buffer for timing issues
    cluster_tests::sleep_n_epochs(
        num_warmup_epochs + 1.0,
        &cluster.genesis_block.poh_config,
        num_ticks_per_slot,
        num_slots_per_epoch,
    );

    // Start up a new node, wait for catchup. Backwards repair won't be sufficient because the
    // leader is sending blobs past this validator's first two confirmed epochs. Thus, the repairman
    // protocol will have to kick in for this validator to repair.

    cluster.add_validator(&validator_config, repairee_stake);

    let all_pubkeys = cluster.get_node_pubkeys();
    let repairee_id = all_pubkeys
        .into_iter()
        .find(|x| !repairman_pubkeys.contains(x))
        .unwrap();

    // Wait for repairman protocol to catch this validator up
    cluster_tests::sleep_n_epochs(
        num_warmup_epochs + 1.0,
        &cluster.genesis_block.poh_config,
        num_ticks_per_slot,
        num_slots_per_epoch,
    );

    cluster.close_preserve_ledgers();
    let validator_ledger_path = cluster.fullnode_infos[&repairee_id].ledger_path.clone();

    // Expect at least the the first two epochs to have been rooted after waiting 3 epochs.
    let num_expected_slots = num_slots_per_epoch * 2;
    let validator_ledger = Blocktree::open(&validator_ledger_path).unwrap();
    let validator_rooted_slots: Vec<_> =
        validator_ledger.rooted_slot_iterator(0).unwrap().collect();

    if validator_rooted_slots.len() as u64 <= num_expected_slots {
        error!(
            "Num expected slots: {}, number of rooted slots: {}",
            num_expected_slots,
            validator_rooted_slots.len()
        );
    }
    assert!(validator_rooted_slots.len() as u64 > num_expected_slots);
}

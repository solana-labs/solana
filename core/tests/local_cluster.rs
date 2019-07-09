extern crate solana;

use hashbrown::HashSet;
use log::*;
use serial_test_derive::serial;
use solana::broadcast_stage::BroadcastStageType;
use solana::cluster::Cluster;
use solana::cluster_tests;
use solana::gossip_service::discover_cluster;
use solana::local_cluster::{ClusterConfig, LocalCluster};
use solana::validator::ValidatorConfig;
use solana_runtime::epoch_schedule::{EpochSchedule, MINIMUM_SLOTS_PER_EPOCH};
use solana_sdk::client::SyncClient;
use solana_sdk::poh_config::PohConfig;
use solana_sdk::timing;
use std::thread::sleep;
use std::time::Duration;

#[test]
#[serial]
fn test_spend_and_verify_all_nodes_1() {
    solana_logger::setup();
    let num_nodes = 1;
    let local = LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
        HashSet::new(),
    );
}

#[test]
#[serial]
fn test_spend_and_verify_all_nodes_2() {
    solana_logger::setup();
    let num_nodes = 2;
    let local = LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
        HashSet::new(),
    );
}

#[test]
#[serial]
fn test_spend_and_verify_all_nodes_3() {
    solana_logger::setup();
    let num_nodes = 3;
    let local = LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
        HashSet::new(),
    );
}

#[allow(unused_attributes)]
#[test]
#[serial]
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
        HashSet::new(),
    );
}

#[allow(unused_attributes)]
#[test]
#[serial]
#[should_panic]
fn test_fullnode_exit_default_config_should_panic() {
    solana_logger::setup();
    let num_nodes = 2;
    let local = LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100);
    cluster_tests::fullnode_exit(&local.entry_point_info, num_nodes);
}

#[test]
#[serial]
fn test_fullnode_exit_2() {
    solana_logger::setup();
    let num_nodes = 2;
    let mut validator_config = ValidatorConfig::default();
    validator_config.rpc_config.enable_fullnode_exit = true;
    let config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100; num_nodes],
        validator_configs: vec![validator_config.clone(); num_nodes],
        ..ClusterConfig::default()
    };
    let local = LocalCluster::new(&config);
    cluster_tests::fullnode_exit(&local.entry_point_info, num_nodes);
}

// Cluster needs a supermajority to remain, so the minimum size for this test is 4
#[test]
#[serial]
fn test_leader_failure_4() {
    solana_logger::setup();
    let num_nodes = 4;
    let mut validator_config = ValidatorConfig::default();
    validator_config.rpc_config.enable_fullnode_exit = true;
    let config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100; 4],
        validator_configs: vec![validator_config.clone(); num_nodes],
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
#[serial]
fn test_two_unbalanced_stakes() {
    solana_logger::setup();
    let mut validator_config = ValidatorConfig::default();
    let num_ticks_per_second = 100;
    let num_ticks_per_slot = 10;
    let num_slots_per_epoch = MINIMUM_SLOTS_PER_EPOCH as u64;

    validator_config.rpc_config.enable_fullnode_exit = true;
    let mut cluster = LocalCluster::new(&ClusterConfig {
        node_stakes: vec![999_990, 3],
        cluster_lamports: 1_000_000,
        validator_configs: vec![validator_config.clone(); 2],
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
    let leader_ledger = cluster.fullnode_infos[&leader_pubkey]
        .info
        .ledger_path
        .clone();
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
        validator_configs: vec![ValidatorConfig::default(); 3],
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
#[serial]
fn test_restart_node() {
    let slots_per_epoch = MINIMUM_SLOTS_PER_EPOCH as u64;
    let ticks_per_slot = 16;
    let mut cluster = LocalCluster::new(&ClusterConfig {
        node_stakes: vec![3],
        cluster_lamports: 100,
        validator_configs: vec![ValidatorConfig::default()],
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
#[serial]
fn test_listener_startup() {
    let config = ClusterConfig {
        node_stakes: vec![100; 1],
        cluster_lamports: 1_000,
        num_listeners: 3,
        validator_configs: vec![ValidatorConfig::default(); 1],
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&config);
    let (cluster_nodes, _) = discover_cluster(&cluster.entry_point_info.gossip, 4).unwrap();
    assert_eq!(cluster_nodes.len(), 4);
}

#[test]
#[serial]
fn test_fail_entry_verification_leader() {
    test_faulty_node(BroadcastStageType::FailEntryVerification);
}

#[test]
#[serial]
fn test_bad_blob_size_leader() {
    test_faulty_node(BroadcastStageType::BroadcastBadBlobSizes);
}

#[test]
#[ignore]
fn test_fake_blobs_broadcast_leader() {
    test_faulty_node(BroadcastStageType::BroadcastFakeBlobs);
}

fn test_faulty_node(faulty_node_type: BroadcastStageType) {
    solana_logger::setup();
    let num_nodes = 4;
    let validator_config = ValidatorConfig::default();
    let mut error_validator_config = ValidatorConfig::default();
    error_validator_config.broadcast_stage_type = faulty_node_type.clone();
    let mut validator_configs = vec![validator_config; num_nodes - 1];
    validator_configs.push(error_validator_config);
    let mut node_stakes = vec![100; num_nodes - 1];
    node_stakes.push(50);
    let cluster_config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes,
        validator_configs: validator_configs,
        slots_per_epoch: MINIMUM_SLOTS_PER_EPOCH * 2 as u64,
        stakers_slot_offset: MINIMUM_SLOTS_PER_EPOCH * 2 as u64,
        ..ClusterConfig::default()
    };

    let cluster = LocalCluster::new(&cluster_config);
    let epoch_schedule = EpochSchedule::new(
        cluster_config.slots_per_epoch,
        cluster_config.stakers_slot_offset,
        true,
    );
    let num_warmup_epochs = epoch_schedule.get_stakers_epoch(0) + 1;

    // Wait for the corrupted leader to be scheduled afer the warmup epochs expire
    cluster_tests::sleep_n_epochs(
        (num_warmup_epochs + 1) as f64,
        &cluster.genesis_block.poh_config,
        cluster_config.ticks_per_slot,
        cluster_config.slots_per_epoch,
    );

    let corrupt_node = cluster
        .fullnode_infos
        .iter()
        .find(|(_, v)| v.config.broadcast_stage_type == faulty_node_type)
        .unwrap()
        .0;
    let mut ignore = HashSet::new();
    ignore.insert(*corrupt_node);

    // Verify that we can still spend and verify even in the presence of corrupt nodes
    cluster_tests::spend_and_verify_all_nodes(
        &cluster.entry_point_info,
        &cluster.funding_keypair,
        num_nodes,
        ignore,
    );
}

#[test]
#[serial]
fn test_repairman_catchup() {
    solana_logger::setup();
    run_repairman_catchup(3);
}

fn run_repairman_catchup(num_repairmen: u64) {
    let mut validator_config = ValidatorConfig::default();
    let num_ticks_per_second = 100;
    let num_ticks_per_slot = 40;
    let num_slots_per_epoch = MINIMUM_SLOTS_PER_EPOCH as u64;
    let num_root_buffer_slots = 10;
    // Calculate the leader schedule num_root_buffer_slots ahead. Otherwise, if stakers_slot_offset ==
    // num_slots_per_epoch, and num_slots_per_epoch == MINIMUM_SLOTS_PER_EPOCH, then repairmen
    // will stop sending repairs after the last slot in epoch 1 (0-indexed), because the root
    // is at most in the first epoch.
    //
    // For example:
    // Assume:
    // 1) num_slots_per_epoch = 32
    // 2) stakers_slot_offset = 32
    // 3) MINIMUM_SLOTS_PER_EPOCH = 32
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
        validator_configs: vec![validator_config.clone(); num_repairmen as usize],
        ticks_per_slot: num_ticks_per_slot,
        slots_per_epoch: num_slots_per_epoch,
        stakers_slot_offset,
        poh_config: PohConfig::new_sleep(Duration::from_millis(1000 / num_ticks_per_second)),
        ..ClusterConfig::default()
    });

    let repairman_pubkeys: HashSet<_> = cluster.get_node_pubkeys().into_iter().collect();
    let epoch_schedule = EpochSchedule::new(num_slots_per_epoch, stakers_slot_offset, true);
    let num_warmup_epochs = epoch_schedule.get_stakers_epoch(0) + 1;

    // Sleep for longer than the first N warmup epochs, with a one epoch buffer for timing issues
    cluster_tests::sleep_n_epochs(
        num_warmup_epochs as f64 + 1.0,
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
    let repairee_client = cluster.get_validator_client(&repairee_id).unwrap();
    let mut current_slot = 0;

    // Make sure this validator can get repaired past the first few warmup epochs
    let target_slot = (num_warmup_epochs) * num_slots_per_epoch + 1;
    while current_slot <= target_slot {
        trace!("current_slot: {}", current_slot);
        if let Ok(slot) = repairee_client.get_slot() {
            current_slot = slot;
        } else {
            continue;
        }
        sleep(Duration::from_secs(1));
    }
}

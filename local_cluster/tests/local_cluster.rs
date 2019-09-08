extern crate solana_core;

use log::*;
use serial_test_derive::serial;
use solana_core::{
    bank_forks::SnapshotConfig, blocktree::Blocktree, broadcast_stage::BroadcastStageType,
    gossip_service::discover_cluster, snapshot_utils, validator::ValidatorConfig,
};
use solana_local_cluster::cluster::Cluster;
use solana_local_cluster::{
    cluster_tests,
    local_cluster::{ClusterConfig, LocalCluster},
};
use solana_runtime::{
    accounts_db::AccountsDB,
    epoch_schedule::{EpochSchedule, MINIMUM_SLOTS_PER_EPOCH},
};
use solana_sdk::{client::SyncClient, clock, poh_config::PohConfig};
use std::path::PathBuf;
use std::{
    collections::{HashMap, HashSet},
    fs,
    thread::sleep,
    time::Duration,
};
use tempfile::TempDir;

#[test]
#[serial]
#[allow(unused_attributes)]
#[ignore]
fn test_ledger_cleanup_service() {
    solana_logger::setup();
    error!("test_ledger_cleanup_service");
    let num_nodes = 3;
    let mut validator_config = ValidatorConfig::default();
    validator_config.max_ledger_slots = Some(100);
    let config = ClusterConfig {
        cluster_lamports: 10_000,
        poh_config: PohConfig::new_sleep(Duration::from_millis(50)),
        node_stakes: vec![100; num_nodes],
        validator_configs: vec![validator_config.clone(); num_nodes],
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&config);
    // 200ms/per * 100 = 20 seconds, so sleep a little longer than that.
    sleep(Duration::from_secs(60));

    cluster_tests::spend_and_verify_all_nodes(
        &cluster.entry_point_info,
        &cluster.funding_keypair,
        num_nodes,
        HashSet::new(),
    );
    cluster.close_preserve_ledgers();
    //check everyone's ledgers and make sure only ~100 slots are stored
    for (_, info) in &cluster.fullnode_infos {
        let mut slots = 0;
        let blocktree = Blocktree::open(&info.info.ledger_path).unwrap();
        blocktree
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|_| slots += 1);
        // with 3 nodes upto 3 slots can be in progress and not complete so max slots in blocktree should be upto 103
        assert!(slots <= 103, "got {}", slots);
    }
}

#[test]
#[serial]
fn test_spend_and_verify_all_nodes_1() {
    solana_logger::setup();
    error!("test_spend_and_verify_all_nodes_1");
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
    error!("test_spend_and_verify_all_nodes_2");
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
    error!("test_spend_and_verify_all_nodes_3");
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
    error!("test_fullnode_exit_default_config_should_panic");
    let num_nodes = 2;
    let local = LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100);
    cluster_tests::fullnode_exit(&local.entry_point_info, num_nodes);
}

#[test]
#[serial]
fn test_fullnode_exit_2() {
    solana_logger::setup();
    error!("test_fullnode_exit_2");
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
#[allow(unused_attributes)]
#[test]
#[serial]
#[ignore]
fn test_leader_failure_4() {
    solana_logger::setup();
    error!("test_leader_failure_4");
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
    error!("test_two_unbalanced_stakes");
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
    cluster_tests::send_many_transactions(&validator_info, &cluster.funding_keypair, 10, 20);
}

#[test]
#[serial]
fn test_restart_node() {
    solana_logger::setup();
    error!("test_restart_node");
    let slots_per_epoch = MINIMUM_SLOTS_PER_EPOCH as u64;
    let ticks_per_slot = 16;
    let validator_config = ValidatorConfig::default();
    let mut cluster = LocalCluster::new(&ClusterConfig {
        node_stakes: vec![3],
        cluster_lamports: 100,
        validator_configs: vec![validator_config.clone()],
        ticks_per_slot,
        slots_per_epoch,
        ..ClusterConfig::default()
    });
    let nodes = cluster.get_node_pubkeys();
    cluster_tests::sleep_n_epochs(
        1.0,
        &cluster.genesis_block.poh_config,
        clock::DEFAULT_TICKS_PER_SLOT,
        slots_per_epoch,
    );
    cluster.exit_restart_node(&nodes[0], validator_config);
    cluster_tests::sleep_n_epochs(
        0.5,
        &cluster.genesis_block.poh_config,
        clock::DEFAULT_TICKS_PER_SLOT,
        slots_per_epoch,
    );
    cluster_tests::send_many_transactions(
        &cluster.entry_point_info,
        &cluster.funding_keypair,
        10,
        1,
    );
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

/*#[allow(unused_attributes)]
#[test]
#[serial]
fn test_snapshot_restart_locktower() {
    // First set up the cluster with 2 nodes
    let snapshot_interval_slots = 10;
    let num_account_paths = 4;

    let leader_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
    let validator_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);

    let snapshot_package_output_path = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .as_ref()
        .unwrap()
        .snapshot_package_output_path;

    let config = ClusterConfig {
        node_stakes: vec![10000],
        cluster_lamports: 100000,
        validator_configs: vec![leader_snapshot_test_config.validator_config.clone()],
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&config);

    // Let the nodes run for a while, then stop one of the validators
    sleep(Duration::from_millis(5000));
    cluster_tests::fullnode_exit(&local.entry_point_info, num_nodes);

    trace!("Waiting for snapshot tar to be generated with slot",);
    let tar = snapshot_utils::get_snapshot_tar_path(&snapshot_package_output_path);
    loop {
        if tar.exists() {
            trace!("snapshot tar exists");
            break;
        }
        sleep(Duration::from_millis(5000));
    }

    // Copy tar to validator's snapshot output directory
    let validator_tar_path =
        snapshot_utils::get_snapshot_tar_path(&validator_snapshot_test_config.snapshot_output_path);
    fs::hard_link(tar, &validator_tar_path).unwrap();
    let slot_floor = snapshot_utils::bank_slot_from_archive(&validator_tar_path).unwrap();

    // Start up a new node from a snapshot
    let validator_stake = 5;
    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        validator_stake,
    );
    let all_pubkeys = cluster.get_node_pubkeys();
    let validator_id = all_pubkeys
        .into_iter()
        .find(|x| *x != cluster.entry_point_info.id)
        .unwrap();
    let validator_client = cluster.get_validator_client(&validator_id).unwrap();
    let mut current_slot = 0;

    // Let this validator run a while with repair
    let target_slot = slot_floor + 40;
    while current_slot <= target_slot {
        trace!("current_slot: {}", current_slot);
        if let Ok(slot) = validator_client.get_slot() {
            current_slot = slot;
        } else {
            continue;
        }
        sleep(Duration::from_secs(1));
    }

    // Check the validator ledger doesn't contain any slots < slot_floor
    cluster.close_preserve_ledgers();
    let validator_ledger_path = &cluster.fullnode_infos[&validator_id];
    let blocktree = Blocktree::open(&validator_ledger_path.info.ledger_path).unwrap();

    // Skip the zeroth slot in blocktree that the ledger is initialized with
    let (first_slot, _) = blocktree.slot_meta_iterator(1).unwrap().next().unwrap();

    assert_eq!(first_slot, slot_floor);
}*/

#[allow(unused_attributes)]
#[test]
#[serial]
#[ignore]
fn test_snapshots_blocktree_floor() {
    // First set up the cluster with 1 snapshotting leader
    let snapshot_interval_slots = 10;
    let num_account_paths = 4;

    let leader_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
    let validator_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);

    let snapshot_package_output_path = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .as_ref()
        .unwrap()
        .snapshot_package_output_path;

    let config = ClusterConfig {
        node_stakes: vec![10000],
        cluster_lamports: 100000,
        validator_configs: vec![leader_snapshot_test_config.validator_config.clone()],
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&config);

    trace!("Waiting for snapshot tar to be generated with slot",);

    let tar = snapshot_utils::get_snapshot_tar_path(&snapshot_package_output_path);
    loop {
        if tar.exists() {
            trace!("snapshot tar exists");
            break;
        }
        sleep(Duration::from_millis(5000));
    }

    // Copy tar to validator's snapshot output directory
    let validator_tar_path =
        snapshot_utils::get_snapshot_tar_path(&validator_snapshot_test_config.snapshot_output_path);
    fs::hard_link(tar, &validator_tar_path).unwrap();
    let slot_floor = snapshot_utils::bank_slot_from_archive(&validator_tar_path).unwrap();

    // Start up a new node from a snapshot
    let validator_stake = 5;
    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        validator_stake,
    );
    let all_pubkeys = cluster.get_node_pubkeys();
    let validator_id = all_pubkeys
        .into_iter()
        .find(|x| *x != cluster.entry_point_info.id)
        .unwrap();
    let validator_client = cluster.get_validator_client(&validator_id).unwrap();
    let mut current_slot = 0;

    // Let this validator run a while with repair
    let target_slot = slot_floor + 40;
    while current_slot <= target_slot {
        trace!("current_slot: {}", current_slot);
        if let Ok(slot) = validator_client.get_slot() {
            current_slot = slot;
        } else {
            continue;
        }
        sleep(Duration::from_secs(1));
    }

    // Check the validator ledger doesn't contain any slots < slot_floor
    cluster.close_preserve_ledgers();
    let validator_ledger_path = &cluster.fullnode_infos[&validator_id];
    let blocktree = Blocktree::open(&validator_ledger_path.info.ledger_path).unwrap();

    // Skip the zeroth slot in blocktree that the ledger is initialized with
    let (first_slot, _) = blocktree.slot_meta_iterator(1).unwrap().next().unwrap();

    assert_eq!(first_slot, slot_floor);
}

#[test]
#[serial]
fn test_snapshots_restart_validity() {
    solana_logger::setup();
    let snapshot_interval_slots = 10;
    let num_account_paths = 4;
    let mut snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
    let snapshot_package_output_path = &snapshot_test_config
        .validator_config
        .snapshot_config
        .as_ref()
        .unwrap()
        .snapshot_package_output_path;

    // Set up the cluster with 1 snapshotting validator
    let mut all_account_storage_dirs = vec![vec![]];

    std::mem::swap(
        &mut all_account_storage_dirs[0],
        &mut snapshot_test_config.account_storage_dirs,
    );

    let config = ClusterConfig {
        node_stakes: vec![10000],
        cluster_lamports: 100000,
        validator_configs: vec![snapshot_test_config.validator_config.clone()],
        ..ClusterConfig::default()
    };

    // Create and reboot the node from snapshot `num_runs` times
    let num_runs = 3;
    let mut expected_balances = HashMap::new();
    let mut cluster = LocalCluster::new(&config);
    for i in 1..num_runs {
        info!("run {}", i);
        // Push transactions to one of the nodes and confirm that transactions were
        // forwarded to and processed.
        trace!("Sending transactions");
        let new_balances = cluster_tests::send_many_transactions(
            &cluster.entry_point_info,
            &cluster.funding_keypair,
            10,
            10,
        );

        expected_balances.extend(new_balances);

        // Get slot after which this was generated
        let client = cluster
            .get_validator_client(&cluster.entry_point_info.id)
            .unwrap();
        let last_slot = client.get_slot().expect("Couldn't get slot");

        // Wait for a snapshot for a bank >= last_slot to be made so we know that the snapshot
        // must include the transactions just pushed
        let tar = snapshot_utils::get_snapshot_tar_path(&snapshot_package_output_path);
        trace!(
            "Waiting for snapshot tar to be generated with slot > {}",
            last_slot
        );
        loop {
            if tar.exists() {
                trace!("snapshot tar exists");
                let slot = snapshot_utils::bank_slot_from_archive(&tar).unwrap();
                if slot >= last_slot {
                    break;
                }
                trace!("snapshot tar slot {} < last_slot {}", slot, last_slot);
            }
            sleep(Duration::from_millis(5000));
        }

        // Create new account paths since fullnode exit is not guaranteed to cleanup RPC threads,
        // which may delete the old accounts on exit at any point
        let (new_account_storage_dirs, new_account_storage_paths) =
            generate_account_paths(num_account_paths);
        all_account_storage_dirs.push(new_account_storage_dirs);
        snapshot_test_config.validator_config.account_paths = Some(new_account_storage_paths);

        // Restart a node
        trace!("Restarting cluster from snapshot");
        let nodes = cluster.get_node_pubkeys();
        cluster.exit_restart_node(&nodes[0], snapshot_test_config.validator_config.clone());

        // Verify account balances on validator
        trace!("Verifying balances");
        cluster_tests::verify_balances(expected_balances.clone(), &cluster.entry_point_info);

        // Check that we can still push transactions
        trace!("Spending and verifying");
        cluster_tests::spend_and_verify_all_nodes(
            &cluster.entry_point_info,
            &cluster.funding_keypair,
            1,
            HashSet::new(),
        );
    }
}

#[allow(unused_attributes)]
#[test]
#[serial]
#[ignore]
fn test_fail_entry_verification_leader() {
    test_faulty_node(BroadcastStageType::FailEntryVerification);
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

#[allow(unused_attributes)]
#[test]
#[serial]
#[ignore]
fn test_repairman_catchup() {
    solana_logger::setup();
    error!("test_repairman_catchup");
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

fn generate_account_paths(num_account_paths: usize) -> (Vec<TempDir>, String) {
    let account_storage_dirs: Vec<TempDir> = (0..num_account_paths)
        .map(|_| TempDir::new().unwrap())
        .collect();
    let account_storage_paths: Vec<_> = account_storage_dirs
        .iter()
        .map(|a| a.path().to_str().unwrap().to_string())
        .collect();
    let account_storage_paths = AccountsDB::format_paths(account_storage_paths);
    (account_storage_dirs, account_storage_paths)
}

struct SnapshotValidatorConfig {
    _snapshot_dir: TempDir,
    snapshot_output_path: TempDir,
    account_storage_dirs: Vec<TempDir>,
    validator_config: ValidatorConfig,
}

fn setup_snapshot_validator_config(
    snapshot_interval_slots: usize,
    num_account_paths: usize,
) -> SnapshotValidatorConfig {
    // Create the snapshot config
    let snapshot_dir = TempDir::new().unwrap();
    let snapshot_output_path = TempDir::new().unwrap();
    let snapshot_config = SnapshotConfig {
        snapshot_interval_slots,
        snapshot_package_output_path: PathBuf::from(snapshot_output_path.path()),
        snapshot_path: PathBuf::from(snapshot_dir.path()),
    };

    // Create the account paths
    let (account_storage_dirs, account_storage_paths) = generate_account_paths(num_account_paths);

    // Create the validator config
    let mut validator_config = ValidatorConfig::default();
    validator_config.rpc_config.enable_fullnode_exit = true;
    validator_config.snapshot_config = Some(snapshot_config);
    validator_config.account_paths = Some(account_storage_paths);

    SnapshotValidatorConfig {
        _snapshot_dir: snapshot_dir,
        snapshot_output_path,
        account_storage_dirs,
        validator_config,
    }
}

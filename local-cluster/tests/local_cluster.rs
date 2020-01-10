use assert_matches::assert_matches;
use log::*;
use serial_test_derive::serial;
use solana_client::thin_client::create_client;
use solana_core::{
    broadcast_stage::BroadcastStageType,
    consensus::VOTE_THRESHOLD_DEPTH,
    gossip_service::discover_cluster,
    partition_cfg::{Partition, PartitionCfg},
    validator::ValidatorConfig,
};
use solana_ledger::{
    bank_forks::SnapshotConfig, blockstore::Blockstore, leader_schedule::FixedSchedule,
    leader_schedule::LeaderSchedule, snapshot_utils,
};
use solana_local_cluster::{
    cluster::Cluster,
    cluster_tests,
    local_cluster::{ClusterConfig, LocalCluster},
};
use solana_sdk::timing::timestamp;
use solana_sdk::{
    client::SyncClient,
    clock,
    commitment_config::CommitmentConfig,
    epoch_schedule::{EpochSchedule, MINIMUM_SLOTS_PER_EPOCH},
    genesis_config::OperatingMode,
    poh_config::PohConfig,
    signature::{Keypair, KeypairUtil},
};
use std::{
    collections::{HashMap, HashSet},
    fs, iter,
    path::{Path, PathBuf},
    sync::Arc,
    thread::sleep,
    time::{Duration, Instant},
};
use tempfile::TempDir;

#[test]
#[serial]
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
    for (_, info) in &cluster.validators {
        let mut slots = 0;
        let blockstore = Blockstore::open(&info.info.ledger_path).unwrap();
        blockstore
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|_| slots += 1);
        // with 3 nodes upto 3 slots can be in progress and not complete so max slots in blockstore should be upto 103
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

#[test]
#[allow(unused_attributes)]
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
#[should_panic]
fn test_validator_exit_default_config_should_panic() {
    solana_logger::setup();
    error!("test_validator_exit_default_config_should_panic");
    let num_nodes = 2;
    let local = LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100);
    cluster_tests::validator_exit(&local.entry_point_info, num_nodes);
}

#[test]
#[serial]
fn test_validator_exit_2() {
    solana_logger::setup();
    error!("test_validator_exit_2");
    let num_nodes = 2;
    let mut validator_config = ValidatorConfig::default();
    validator_config.rpc_config.enable_validator_exit = true;
    validator_config.wait_for_supermajority = true;

    let config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100; num_nodes],
        validator_configs: vec![validator_config.clone(); num_nodes],
        ..ClusterConfig::default()
    };
    let local = LocalCluster::new(&config);
    cluster_tests::validator_exit(&local.entry_point_info, num_nodes);
}

// Cluster needs a supermajority to remain, so the minimum size for this test is 4
#[test]
#[serial]
fn test_leader_failure_4() {
    solana_logger::setup();
    error!("test_leader_failure_4");
    let num_nodes = 4;
    let mut validator_config = ValidatorConfig::default();
    validator_config.rpc_config.enable_validator_exit = true;
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

/// This function runs a network, initiates a partition based on a
/// configuration, resolve the partition, then checks that the network
/// continues to achieve consensus
/// # Arguments
/// * `partitions` - A slice of partition configurations, where each partition
/// configuration is a slice of (usize, bool), representing a node's stake and
/// whether or not it should be killed during the partition
/// * `leader_schedule` - An option that specifies whether the cluster should
/// run with a fixed, predetermined leader schedule
fn run_cluster_partition(
    partitions: &[&[(usize, bool)]],
    leader_schedule: Option<(LeaderSchedule, Vec<Arc<Keypair>>)>,
) {
    solana_logger::setup();
    info!("PARTITION_TEST!");
    let num_nodes = partitions.len();
    let node_stakes: Vec<_> = partitions
        .iter()
        .flat_map(|p| p.iter().map(|(stake_weight, _)| 100 * *stake_weight as u64))
        .collect();
    assert_eq!(node_stakes.len(), num_nodes);
    let cluster_lamports = node_stakes.iter().sum::<u64>() * 2;
    let partition_start_epoch = 2;
    let mut validator_config = ValidatorConfig::default();

    // Returns:
    // 1) The keys for the validiators
    // 2) The amount of time it would take to iterate through one full iteration of the given
    // leader schedule
    let (validator_keys, leader_schedule_time): (Vec<_>, u64) = {
        if let Some((leader_schedule, validator_keys)) = leader_schedule {
            assert_eq!(validator_keys.len(), num_nodes);
            let num_slots_per_rotation = leader_schedule.num_slots() as u64;
            let fixed_schedule = FixedSchedule {
                start_epoch: partition_start_epoch,
                leader_schedule: Arc::new(leader_schedule),
            };
            validator_config.fixed_leader_schedule = Some(fixed_schedule);
            (
                validator_keys,
                num_slots_per_rotation * clock::DEFAULT_MS_PER_SLOT,
            )
        } else {
            (
                iter::repeat_with(|| Arc::new(Keypair::new()))
                    .take(partitions.len())
                    .collect(),
                10_000,
            )
        }
    };

    let validator_pubkeys: Vec<_> = validator_keys.iter().map(|v| v.pubkey()).collect();
    let mut config = ClusterConfig {
        cluster_lamports,
        node_stakes,
        validator_configs: vec![validator_config.clone(); num_nodes],
        validator_keys: Some(validator_keys),
        ..ClusterConfig::default()
    };

    let now = timestamp();
    // Partition needs to start after the first few shorter warmup epochs, otherwise
    // no root will be set before the partition is resolved, the leader schedule will
    // not be computable, and the cluster wll halt.
    let partition_epoch_start_offset = cluster_tests::time_until_nth_epoch(
        partition_start_epoch,
        config.slots_per_epoch,
        config.stakers_slot_offset,
    );
    // Assume it takes <= 10 seconds for `LocalCluster::new` to boot up.
    let local_cluster_boot_time = 10_000;
    let partition_start = now + partition_epoch_start_offset + local_cluster_boot_time;
    let partition_end = partition_start + leader_schedule_time as u64;
    let mut validator_index = 0;
    for (i, partition) in partitions.iter().enumerate() {
        for _ in partition.iter() {
            let mut p1 = Partition::default();
            p1.num_partitions = partitions.len();
            p1.my_partition = i;
            p1.start_ts = partition_start;
            p1.end_ts = partition_end;
            config.validator_configs[validator_index].partition_cfg =
                Some(PartitionCfg::new(vec![p1]));
            validator_index += 1;
        }
    }
    info!(
        "PARTITION_TEST starting cluster with {:?} partitions",
        partitions
    );
    let now = Instant::now();
    let mut cluster = LocalCluster::new(&config);
    let elapsed = now.elapsed();
    assert!(elapsed.as_millis() < local_cluster_boot_time as u128);

    let now = timestamp();
    let timeout = partition_start as u64 - now as u64;
    info!(
        "PARTITION_TEST sleeping until partition start timeout {}",
        timeout
    );
    let mut dead_nodes = HashSet::new();
    if timeout > 0 {
        sleep(Duration::from_millis(timeout as u64));
    }
    info!("PARTITION_TEST done sleeping until partition start timeout");
    let now = timestamp();
    let timeout = partition_end as u64 - now as u64;
    info!(
        "PARTITION_TEST sleeping until partition end timeout {}",
        timeout
    );
    let mut alive_node_contact_infos = vec![];
    let should_exits: Vec<_> = partitions
        .iter()
        .flat_map(|p| p.iter().map(|(_, should_exit)| should_exit))
        .collect();
    assert_eq!(should_exits.len(), validator_pubkeys.len());
    if timeout > 0 {
        // Give partitions time to propagate their blocks from durinig the partition
        // after the partition resolves
        let propagation_time = leader_schedule_time;
        info!("PARTITION_TEST resolving partition");
        sleep(Duration::from_millis(timeout));
        info!("PARTITION_TEST waiting for blocks to propagate after partition");
        sleep(Duration::from_millis(propagation_time));
        info!("PARTITION_TEST resuming normal operation");
        for (pubkey, should_exit) in validator_pubkeys.iter().zip(should_exits) {
            if *should_exit {
                info!("Killing validator with id: {}", pubkey);
                cluster.exit_node(pubkey);
                dead_nodes.insert(*pubkey);
            } else {
                alive_node_contact_infos.push(
                    cluster
                        .validators
                        .get(pubkey)
                        .unwrap()
                        .info
                        .contact_info
                        .clone(),
                );
            }
        }
    }

    assert!(alive_node_contact_infos.len() > 0);
    info!("PARTITION_TEST discovering nodes");
    let (cluster_nodes, _) = discover_cluster(
        &alive_node_contact_infos[0].gossip,
        alive_node_contact_infos.len(),
    )
    .unwrap();
    info!("PARTITION_TEST discovered {} nodes", cluster_nodes.len());
    info!("PARTITION_TEST looking for new roots on all nodes");
    let mut roots = vec![HashSet::new(); alive_node_contact_infos.len()];
    let mut done = false;
    while !done {
        for (i, ingress_node) in alive_node_contact_infos.iter().enumerate() {
            let client = create_client(
                ingress_node.client_facing_addr(),
                solana_core::cluster_info::VALIDATOR_PORT_RANGE,
            );
            let slot = client.get_slot().unwrap_or(0);
            roots[i].insert(slot);
            let min_node = roots.iter().map(|r| r.len()).min().unwrap_or(0);
            info!("PARTITION_TEST min observed roots {}/16", min_node);
            done = min_node >= 16;
        }
        sleep(Duration::from_millis(clock::DEFAULT_MS_PER_SLOT / 2));
    }
    info!("PARTITION_TEST done spending on all node");
}

#[allow(unused_attributes)]
#[ignore]
#[test]
#[serial]
fn test_cluster_partition_1_2() {
    run_cluster_partition(&[&[(1, false)], &[(1, false), (1, false)]], None)
}

#[allow(unused_attributes)]
#[ignore]
#[test]
#[serial]
fn test_cluster_partition_1_1() {
    run_cluster_partition(&[&[(1, false)], &[(1, false)]], None)
}

#[test]
#[serial]
fn test_cluster_partition_1_1_1() {
    run_cluster_partition(&[&[(1, false)], &[(1, false)], &[(1, false)]], None)
}

#[test]
#[serial]
fn test_kill_partition() {
    // This test:
    // 1) Spins up three partitions
    // 2) Forces more slots in the leader schedule for the first partition so
    // that this partition will be the heaviiest
    // 3) Schedules the other validators for sufficient slots in the schedule
    // so that they will still be locked out of voting for the major partitoin
    // when the partition resolves
    // 4) Kills the major partition. Validators are locked out, but should be
    // able to reset to the major partition
    // 5) Check for recovery
    let mut leader_schedule = vec![];
    let num_slots_per_validator = 8;
    let partitions: [&[(usize, bool)]; 3] = [&[(9, true)], &[(10, false)], &[(10, false)]];
    let validator_keys: Vec<_> = iter::repeat_with(|| Arc::new(Keypair::new()))
        .take(partitions.len())
        .collect();
    for (i, k) in validator_keys.iter().enumerate() {
        let num_slots = {
            if i == 0 {
                // Set up the leader to have 50% of the slots
                num_slots_per_validator * (partitions.len() - 1)
            } else {
                num_slots_per_validator
            }
        };
        for _ in 0..num_slots {
            leader_schedule.push(k.pubkey())
        }
    }

    run_cluster_partition(
        &partitions,
        Some((
            LeaderSchedule::new_from_schedule(leader_schedule),
            validator_keys,
        )),
    )
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

    validator_config.rpc_config.enable_validator_exit = true;
    let mut cluster = LocalCluster::new(&ClusterConfig {
        node_stakes: vec![999_990, 3],
        cluster_lamports: 1_000_000,
        validator_configs: vec![validator_config.clone(); 2],
        ticks_per_slot: num_ticks_per_slot,
        slots_per_epoch: num_slots_per_epoch,
        stakers_slot_offset: num_slots_per_epoch,
        poh_config: PohConfig::new_sleep(Duration::from_millis(1000 / num_ticks_per_second)),
        ..ClusterConfig::default()
    });

    cluster_tests::sleep_n_epochs(
        10.0,
        &cluster.genesis_config.poh_config,
        num_ticks_per_slot,
        num_slots_per_epoch,
    );
    cluster.close_preserve_ledgers();
    let leader_pubkey = cluster.entry_point_info.id;
    let leader_ledger = cluster.validators[&leader_pubkey].info.ledger_path.clone();
    cluster_tests::verify_ledger_ticks(&leader_ledger, num_ticks_per_slot as usize);
}

#[test]
#[serial]
fn test_forwarding() {
    // Set up a cluster where one node is never the leader, so all txs sent to this node
    // will be have to be forwarded in order to be confirmed
    let config = ClusterConfig {
        node_stakes: vec![999_990, 3],
        cluster_lamports: 2_000_000,
        validator_configs: vec![ValidatorConfig::default(); 2],
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
    let slots_per_epoch = MINIMUM_SLOTS_PER_EPOCH * 2 as u64;
    let ticks_per_slot = 16;
    let validator_config = ValidatorConfig::default();
    let mut cluster = LocalCluster::new(&ClusterConfig {
        node_stakes: vec![100; 1],
        cluster_lamports: 100,
        validator_configs: vec![validator_config.clone()],
        ticks_per_slot,
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        ..ClusterConfig::default()
    });
    let nodes = cluster.get_node_pubkeys();
    cluster_tests::sleep_n_epochs(
        1.0,
        &cluster.genesis_config.poh_config,
        clock::DEFAULT_TICKS_PER_SLOT,
        slots_per_epoch,
    );
    cluster.exit_restart_node(&nodes[0], validator_config);
    cluster_tests::sleep_n_epochs(
        0.5,
        &cluster.genesis_config.poh_config,
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

#[test]
#[serial]
fn test_softlaunch_operating_mode() {
    solana_logger::setup();

    let config = ClusterConfig {
        operating_mode: OperatingMode::SoftLaunch,
        node_stakes: vec![100; 1],
        cluster_lamports: 1_000,
        validator_configs: vec![ValidatorConfig::default(); 1],
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&config);
    let (cluster_nodes, _) = discover_cluster(&cluster.entry_point_info.gossip, 1).unwrap();
    assert_eq!(cluster_nodes.len(), 1);

    let client = create_client(
        cluster.entry_point_info.client_facing_addr(),
        solana_core::cluster_info::VALIDATOR_PORT_RANGE,
    );

    // Programs that are available at soft launch epoch 0
    for program_id in [
        &solana_sdk::system_program::id(),
        &solana_vote_program::id(),
        &solana_stake_program::id(),
    ]
    .iter()
    {
        assert_matches!(
            (
                program_id,
                client
                    .get_account_with_commitment(program_id, CommitmentConfig::recent())
                    .unwrap()
            ),
            (_program_id, Some(_))
        );
    }

    // Programs that are not available at soft launch epoch 0
    for program_id in [
        &solana_config_program::id(),
        &solana_sdk::bpf_loader::id(),
        &solana_storage_program::id(),
        &solana_vest_program::id(),
    ]
    .iter()
    {
        assert_eq!(
            (
                program_id,
                client
                    .get_account_with_commitment(program_id, CommitmentConfig::recent())
                    .unwrap()
            ),
            (program_id, None)
        );
    }
}

#[allow(unused_attributes)]
#[test]
#[serial]
fn test_snapshot_restart_tower() {
    // First set up the cluster with 2 nodes
    let snapshot_interval_slots = 10;
    let num_account_paths = 4;

    let leader_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
    let validator_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);

    let config = ClusterConfig {
        node_stakes: vec![10000, 10],
        cluster_lamports: 100000,
        validator_configs: vec![
            leader_snapshot_test_config.validator_config.clone(),
            validator_snapshot_test_config.validator_config.clone(),
        ],
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&config);

    // Let the nodes run for a while, then stop one of the validators
    sleep(Duration::from_millis(5000));
    let all_pubkeys = cluster.get_node_pubkeys();
    let validator_id = all_pubkeys
        .into_iter()
        .find(|x| *x != cluster.entry_point_info.id)
        .unwrap();
    let validator_info = cluster.exit_node(&validator_id);

    // Get slot after which this was generated
    let snapshot_package_output_path = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .as_ref()
        .unwrap()
        .snapshot_package_output_path;
    let tar = snapshot_utils::get_snapshot_tar_path(&snapshot_package_output_path);
    wait_for_next_snapshot(&cluster, &tar);

    // Copy tar to validator's snapshot output directory
    let validator_tar_path =
        snapshot_utils::get_snapshot_tar_path(&validator_snapshot_test_config.snapshot_output_path);
    fs::hard_link(tar, &validator_tar_path).unwrap();

    // Restart validator from snapshot, the validator's tower state in this snapshot
    // will contain slots < the root bank of the snapshot. Validator should not panic.
    cluster.restart_node(&validator_id, validator_info);

    // Test cluster can still make progress and get confirmations in tower
    cluster_tests::spend_and_verify_all_nodes(
        &cluster.entry_point_info,
        &cluster.funding_keypair,
        1,
        HashSet::new(),
    );
}

#[test]
#[serial]
fn test_snapshots_blockstore_floor() {
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
        Arc::new(Keypair::new()),
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
        if let Ok(slot) = validator_client.get_slot_with_commitment(CommitmentConfig::recent()) {
            current_slot = slot;
        } else {
            continue;
        }
        sleep(Duration::from_secs(1));
    }

    // Check the validator ledger doesn't contain any slots < slot_floor
    cluster.close_preserve_ledgers();
    let validator_ledger_path = &cluster.validators[&validator_id];
    let blockstore = Blockstore::open(&validator_ledger_path.info.ledger_path).unwrap();

    // Skip the zeroth slot in blockstore that the ledger is initialized with
    let (first_slot, _) = blockstore.slot_meta_iterator(1).unwrap().next().unwrap();

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

        let tar = snapshot_utils::get_snapshot_tar_path(&snapshot_package_output_path);
        wait_for_next_snapshot(&cluster, &tar);

        // Create new account paths since validator exit is not guaranteed to cleanup RPC threads,
        // which may delete the old accounts on exit at any point
        let (new_account_storage_dirs, new_account_storage_paths) =
            generate_account_paths(num_account_paths);
        all_account_storage_dirs.push(new_account_storage_dirs);
        snapshot_test_config.validator_config.account_paths = new_account_storage_paths;

        // Restart node
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

#[test]
#[serial]
#[allow(unused_attributes)]
#[ignore]
fn test_fail_entry_verification_leader() {
    test_faulty_node(BroadcastStageType::FailEntryVerification);
}

#[test]
#[allow(unused_attributes)]
#[ignore]
fn test_fake_shreds_broadcast_leader() {
    test_faulty_node(BroadcastStageType::BroadcastFakeShreds);
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
    let epoch_schedule = EpochSchedule::custom(
        cluster_config.slots_per_epoch,
        cluster_config.stakers_slot_offset,
        true,
    );
    let num_warmup_epochs = epoch_schedule.get_leader_schedule_epoch(0) + 1;

    // Wait for the corrupted leader to be scheduled afer the warmup epochs expire
    cluster_tests::sleep_n_epochs(
        (num_warmup_epochs + 1) as f64,
        &cluster.genesis_config.poh_config,
        cluster_config.ticks_per_slot,
        cluster_config.slots_per_epoch,
    );

    let corrupt_node = cluster
        .validators
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
// Test that when a leader is leader for banks B_i..B_{i+n}, and B_i is not
// votable, then B_{i+1} still chains to B_i
fn test_no_voting() {
    solana_logger::setup();
    let mut validator_config = ValidatorConfig::default();
    validator_config.rpc_config.enable_validator_exit = true;
    validator_config.voting_disabled = true;
    let config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100],
        validator_configs: vec![validator_config.clone()],
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&config);
    let client = cluster
        .get_validator_client(&cluster.entry_point_info.id)
        .unwrap();
    loop {
        let last_slot = client
            .get_slot_with_commitment(CommitmentConfig::recent())
            .expect("Couldn't get slot");
        if last_slot > 4 * VOTE_THRESHOLD_DEPTH as u64 {
            break;
        }
        sleep(Duration::from_secs(1));
    }

    cluster.close_preserve_ledgers();
    let leader_pubkey = cluster.entry_point_info.id;
    let ledger_path = cluster.validators[&leader_pubkey].info.ledger_path.clone();
    let ledger = Blockstore::open(&ledger_path).unwrap();
    for i in 0..2 * VOTE_THRESHOLD_DEPTH {
        let meta = ledger.meta(i as u64).unwrap().unwrap();
        let parent = meta.parent_slot;
        let expected_parent = i.saturating_sub(1);
        assert_eq!(parent, expected_parent as u64);
    }
}

#[test]
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

    validator_config.rpc_config.enable_validator_exit = true;

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
    let epoch_schedule = EpochSchedule::custom(num_slots_per_epoch, stakers_slot_offset, true);
    let num_warmup_epochs = epoch_schedule.get_leader_schedule_epoch(0) + 1;

    // Sleep for longer than the first N warmup epochs, with a one epoch buffer for timing issues
    cluster_tests::sleep_n_epochs(
        num_warmup_epochs as f64 + 1.0,
        &cluster.genesis_config.poh_config,
        num_ticks_per_slot,
        num_slots_per_epoch,
    );

    // Start up a new node, wait for catchup. Backwards repair won't be sufficient because the
    // leader is sending shreds past this validator's first two confirmed epochs. Thus, the repairman
    // protocol will have to kick in for this validator to repair.
    cluster.add_validator(&validator_config, repairee_stake, Arc::new(Keypair::new()));

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
        if let Ok(slot) = repairee_client.get_slot_with_commitment(CommitmentConfig::recent()) {
            current_slot = slot;
        } else {
            continue;
        }
        sleep(Duration::from_secs(1));
    }
}

fn wait_for_next_snapshot<P: AsRef<Path>>(cluster: &LocalCluster, tar: P) {
    // Get slot after which this was generated
    let client = cluster
        .get_validator_client(&cluster.entry_point_info.id)
        .unwrap();
    let last_slot = client
        .get_slot_with_commitment(CommitmentConfig::recent())
        .expect("Couldn't get slot");

    // Wait for a snapshot for a bank >= last_slot to be made so we know that the snapshot
    // must include the transactions just pushed
    trace!(
        "Waiting for snapshot tar to be generated with slot > {}",
        last_slot
    );
    loop {
        if tar.as_ref().exists() {
            trace!("snapshot tar exists");
            let slot = snapshot_utils::bank_slot_from_archive(&tar).unwrap();
            if slot >= last_slot {
                break;
            }
            trace!("snapshot tar slot {} < last_slot {}", slot, last_slot);
        }
        sleep(Duration::from_millis(5000));
    }
}

fn generate_account_paths(num_account_paths: usize) -> (Vec<TempDir>, Vec<PathBuf>) {
    let account_storage_dirs: Vec<TempDir> = (0..num_account_paths)
        .map(|_| TempDir::new().unwrap())
        .collect();
    let account_storage_paths: Vec<_> = account_storage_dirs
        .iter()
        .map(|a| a.path().to_path_buf())
        .collect();
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
    validator_config.rpc_config.enable_validator_exit = true;
    validator_config.snapshot_config = Some(snapshot_config);
    validator_config.account_paths = account_storage_paths;

    SnapshotValidatorConfig {
        _snapshot_dir: snapshot_dir,
        snapshot_output_path,
        account_storage_dirs,
        validator_config,
    }
}

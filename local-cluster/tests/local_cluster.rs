use assert_matches::assert_matches;
use log::*;
use serial_test_derive::serial;
use solana_client::rpc_client::RpcClient;
use solana_client::thin_client::create_client;
use solana_core::{
    broadcast_stage::BroadcastStageType, consensus::VOTE_THRESHOLD_DEPTH,
    gossip_service::discover_cluster, validator::ValidatorConfig,
};
use solana_download_utils::download_snapshot;
use solana_ledger::bank_forks::CompressionType;
use solana_ledger::{
    bank_forks::SnapshotConfig, blockstore::Blockstore, leader_schedule::FixedSchedule,
    leader_schedule::LeaderSchedule, snapshot_utils,
};
use solana_local_cluster::{
    cluster::Cluster,
    cluster_tests,
    local_cluster::{ClusterConfig, LocalCluster},
};
use solana_sdk::{
    client::{AsyncClient, SyncClient},
    clock::{self, Slot},
    commitment_config::CommitmentConfig,
    epoch_schedule::{EpochSchedule, MINIMUM_SLOTS_PER_EPOCH},
    genesis_config::OperatingMode,
    hash::Hash,
    poh_config::PohConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::sync::atomic::{AtomicBool, Ordering};
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
    validator_config.max_ledger_shreds = Some(100);
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
    validator_config.wait_for_supermajority = Some(0);

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
    let enable_partition = Arc::new(AtomicBool::new(true));
    let mut validator_config = ValidatorConfig::default();
    validator_config.enable_partition = Some(enable_partition.clone());

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
    let config = ClusterConfig {
        cluster_lamports,
        node_stakes,
        validator_configs: vec![validator_config.clone(); num_nodes],
        validator_keys: Some(validator_keys),
        ..ClusterConfig::default()
    };

    info!(
        "PARTITION_TEST starting cluster with {:?} partitions slots_per_epoch: {}",
        partitions, config.slots_per_epoch,
    );
    let mut cluster = LocalCluster::new(&config);

    let (cluster_nodes, _) = discover_cluster(&cluster.entry_point_info.gossip, num_nodes).unwrap();

    info!("PARTITION_TEST sleeping until partition starting condition",);
    loop {
        let mut reached_epoch = true;
        for node in &cluster_nodes {
            let node_client = RpcClient::new_socket(node.rpc);
            if let Ok(epoch_info) = node_client.get_epoch_info() {
                info!("slots_per_epoch: {:?}", epoch_info);
                if epoch_info.slots_in_epoch <= (1 << VOTE_THRESHOLD_DEPTH) {
                    reached_epoch = false;
                    break;
                }
            } else {
                reached_epoch = false;
            }
        }

        if reached_epoch {
            info!("PARTITION_TEST start partition");
            enable_partition.clone().store(false, Ordering::Relaxed);
            break;
        } else {
            sleep(Duration::from_millis(100));
        }
    }
    sleep(Duration::from_millis(leader_schedule_time));

    info!("PARTITION_TEST remove partition");
    enable_partition.store(true, Ordering::Relaxed);

    let mut dead_nodes = HashSet::new();
    let mut alive_node_contact_infos = vec![];
    let should_exits: Vec<_> = partitions
        .iter()
        .flat_map(|p| p.iter().map(|(_, should_exit)| should_exit))
        .collect();
    assert_eq!(should_exits.len(), validator_pubkeys.len());
    let timeout = 10;
    if timeout > 0 {
        // Give partitions time to propagate their blocks from during the partition
        // after the partition resolves
        let propagation_time = leader_schedule_time;
        info!("PARTITION_TEST resolving partition. sleeping {}ms", timeout);
        sleep(Duration::from_millis(10_000));
        info!(
            "PARTITION_TEST waiting for blocks to propagate after partition {}ms",
            propagation_time
        );
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
    let mut last_print = Instant::now();
    while !done {
        for (i, ingress_node) in alive_node_contact_infos.iter().enumerate() {
            let client = create_client(
                ingress_node.client_facing_addr(),
                solana_core::cluster_info::VALIDATOR_PORT_RANGE,
            );
            let slot = client.get_slot().unwrap_or(0);
            roots[i].insert(slot);
            let min_node = roots.iter().map(|r| r.len()).min().unwrap_or(0);
            if last_print.elapsed().as_secs() > 3 {
                info!("PARTITION_TEST min observed roots {}/16", min_node);
                last_print = Instant::now();
            }
            done = min_node >= 16;
        }
        sleep(Duration::from_millis(clock::DEFAULT_MS_PER_SLOT / 2));
    }
    info!("PARTITION_TEST done waiting for roots");
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
    info!("leader_schedule: {}", leader_schedule.len());

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
fn test_stable_operating_mode() {
    solana_logger::setup();

    let config = ClusterConfig {
        operating_mode: OperatingMode::Stable,
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

    // Programs that are available at epoch 0
    for program_id in [
        &solana_config_program::id(),
        &solana_sdk::system_program::id(),
        &solana_stake_program::id(),
        &solana_vote_program::id(),
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

    // Programs that are not available at epoch 0
    for program_id in [
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

fn generate_frozen_account_panic(mut cluster: LocalCluster, frozen_account: Arc<Keypair>) {
    let client = cluster
        .get_validator_client(&frozen_account.pubkey())
        .unwrap();

    // Check the validator is alive by poking it over RPC
    trace!(
        "validator slot: {}",
        client
            .get_slot_with_commitment(CommitmentConfig::recent())
            .expect("get slot")
    );

    // Reset the frozen account panic signal
    solana_runtime::accounts_db::FROZEN_ACCOUNT_PANIC.store(false, Ordering::Relaxed);

    // Wait for the frozen account panic signal
    let mut i = 0;
    while !solana_runtime::accounts_db::FROZEN_ACCOUNT_PANIC.load(Ordering::Relaxed) {
        // Transfer from frozen account
        let (blockhash, _fee_calculator) = client
            .get_recent_blockhash_with_commitment(CommitmentConfig::recent())
            .unwrap();
        client
            .async_transfer(1, &frozen_account, &Pubkey::new_rand(), blockhash)
            .unwrap();

        sleep(Duration::from_secs(1));
        i += 1;
        if i > 10 {
            panic!("FROZEN_ACCOUNT_PANIC still false");
        }
    }

    // The validator is now broken and won't shutdown properly.  Avoid LocalCluster panic in Drop
    // with some manual cleanup:
    cluster.exit();
    cluster.validators = HashMap::default();
}

#[test]
#[serial]
fn test_frozen_account_from_genesis() {
    solana_logger::setup();
    let validator_identity =
        Arc::new(solana_sdk::signature::keypair_from_seed(&[0u8; 32]).unwrap());

    let config = ClusterConfig {
        validator_keys: Some(vec![validator_identity.clone()]),
        node_stakes: vec![100; 1],
        cluster_lamports: 1_000,
        validator_configs: vec![
            ValidatorConfig {
                // Freeze the validator identity account
                frozen_accounts: vec![validator_identity.pubkey()],
                ..ValidatorConfig::default()
            };
            1
        ],
        ..ClusterConfig::default()
    };
    generate_frozen_account_panic(LocalCluster::new(&config), validator_identity);
}

#[test]
#[serial]
fn test_frozen_account_from_snapshot() {
    solana_logger::setup();
    let validator_identity =
        Arc::new(solana_sdk::signature::keypair_from_seed(&[0u8; 32]).unwrap());

    let mut snapshot_test_config = setup_snapshot_validator_config(5, 1);
    // Freeze the validator identity account
    snapshot_test_config.validator_config.frozen_accounts = vec![validator_identity.pubkey()];

    let config = ClusterConfig {
        validator_keys: Some(vec![validator_identity.clone()]),
        node_stakes: vec![100; 1],
        cluster_lamports: 1_000,
        validator_configs: vec![snapshot_test_config.validator_config.clone()],
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&config);

    let snapshot_package_output_path = &snapshot_test_config
        .validator_config
        .snapshot_config
        .as_ref()
        .unwrap()
        .snapshot_package_output_path;

    trace!("Waiting for snapshot at {:?}", snapshot_package_output_path);
    let (archive_filename, _archive_snapshot_hash) =
        wait_for_next_snapshot(&cluster, &snapshot_package_output_path);

    trace!("Found snapshot: {:?}", archive_filename);

    // Restart the validator from a snapshot
    let validator_info = cluster.exit_node(&validator_identity.pubkey());
    cluster.restart_node(&validator_identity.pubkey(), validator_info);

    generate_frozen_account_panic(cluster, validator_identity);
}

#[test]
#[serial]
fn test_consistency_halt() {
    solana_logger::setup();
    let snapshot_interval_slots = 20;
    let num_account_paths = 1;

    // Create cluster with a leader producing bad snapshot hashes.
    let mut leader_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
    leader_snapshot_test_config
        .validator_config
        .accounts_hash_fault_injection_slots = 40;

    let validator_stake = 10_000;
    let config = ClusterConfig {
        node_stakes: vec![validator_stake],
        cluster_lamports: 100_000,
        validator_configs: vec![leader_snapshot_test_config.validator_config.clone()],
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&config);

    sleep(Duration::from_millis(5000));
    let (cluster_nodes, _) = discover_cluster(&cluster.entry_point_info.gossip, 1).unwrap();
    info!("num_nodes: {}", cluster_nodes.len());

    // Add a validator with the leader as trusted, it should halt when it detects
    // mismatch.
    let mut validator_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);

    let mut trusted_validators = HashSet::new();
    trusted_validators.insert(cluster_nodes[0].id);

    validator_snapshot_test_config
        .validator_config
        .trusted_validators = Some(trusted_validators);
    validator_snapshot_test_config
        .validator_config
        .halt_on_trusted_validators_accounts_hash_mismatch = true;

    warn!("adding a validator");
    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        validator_stake as u64,
        Arc::new(Keypair::new()),
    );
    let num_nodes = 2;
    assert_eq!(
        discover_cluster(&cluster.entry_point_info.gossip, num_nodes)
            .unwrap()
            .0
            .len(),
        num_nodes
    );

    // Check for only 1 node on the network.
    let mut encountered_error = false;
    loop {
        let discover = discover_cluster(&cluster.entry_point_info.gossip, 2);
        match discover {
            Err(_) => {
                encountered_error = true;
                break;
            }
            Ok(nodes) => {
                if nodes.0.len() < 2 {
                    encountered_error = true;
                    break;
                }
                info!("checking cluster for fewer nodes.. {:?}", nodes.0.len());
            }
        }
        let client = cluster
            .get_validator_client(&cluster.entry_point_info.id)
            .unwrap();
        if let Ok(slot) = client.get_slot() {
            if slot > 210 {
                break;
            }
            info!("slot: {}", slot);
        }
        sleep(Duration::from_millis(1000));
    }
    assert!(encountered_error);
}

#[test]
#[serial]
fn test_snapshot_download() {
    solana_logger::setup();
    // First set up the cluster with 1 node
    let snapshot_interval_slots = 50;
    let num_account_paths = 3;

    let leader_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
    let validator_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);

    let stake = 10_000;
    let config = ClusterConfig {
        node_stakes: vec![stake],
        cluster_lamports: 1_000_000,
        validator_configs: vec![leader_snapshot_test_config.validator_config.clone()],
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&config);

    // Get slot after which this was generated
    let snapshot_package_output_path = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .as_ref()
        .unwrap()
        .snapshot_package_output_path;

    trace!("Waiting for snapshot");
    let (archive_filename, archive_snapshot_hash) =
        wait_for_next_snapshot(&cluster, &snapshot_package_output_path);

    trace!("found: {:?}", archive_filename);
    let validator_archive_path = snapshot_utils::get_snapshot_archive_path(
        &validator_snapshot_test_config.snapshot_output_path,
        &archive_snapshot_hash,
        &CompressionType::Bzip2,
    );

    // Download the snapshot, then boot a validator from it.
    download_snapshot(
        &cluster.entry_point_info.rpc,
        &validator_archive_path,
        archive_snapshot_hash,
    )
    .unwrap();

    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        stake,
        Arc::new(Keypair::new()),
    );
}

#[allow(unused_attributes)]
#[test]
#[serial]
fn test_snapshot_restart_tower() {
    // First set up the cluster with 2 nodes
    let snapshot_interval_slots = 10;
    let num_account_paths = 2;

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

    let (archive_filename, archive_snapshot_hash) =
        wait_for_next_snapshot(&cluster, &snapshot_package_output_path);

    // Copy archive to validator's snapshot output directory
    let validator_archive_path = snapshot_utils::get_snapshot_archive_path(
        &validator_snapshot_test_config.snapshot_output_path,
        &archive_snapshot_hash,
        &CompressionType::Bzip2,
    );
    fs::hard_link(archive_filename, &validator_archive_path).unwrap();

    // Restart validator from snapshot, the validator's tower state in this snapshot
    // will contain slots < the root bank of the snapshot. Validator should not panic.
    cluster.restart_node(&validator_id, validator_info);

    // Test cluster can still make progress and get confirmations in tower
    // Use the restarted node as the discovery point so that we get updated
    // validator's ContactInfo
    let restarted_node_info = cluster.get_contact_info(&validator_id).unwrap();
    cluster_tests::spend_and_verify_all_nodes(
        &restarted_node_info,
        &cluster.funding_keypair,
        1,
        HashSet::new(),
    );
}

#[test]
#[serial]
fn test_snapshots_blockstore_floor() {
    solana_logger::setup();
    // First set up the cluster with 1 snapshotting leader
    let snapshot_interval_slots = 10;
    let num_account_paths = 4;

    let leader_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
    let mut validator_snapshot_test_config =
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

    let (archive_filename, (archive_slot, archive_hash, _)) = loop {
        let archive =
            snapshot_utils::get_highest_snapshot_archive_path(&snapshot_package_output_path);
        if archive.is_some() {
            trace!("snapshot exists");
            break archive.unwrap();
        }
        sleep(Duration::from_millis(5000));
    };

    // Copy archive to validator's snapshot output directory
    let validator_archive_path = snapshot_utils::get_snapshot_archive_path(
        &validator_snapshot_test_config.snapshot_output_path,
        &(archive_slot, archive_hash),
        &CompressionType::Bzip2,
    );
    fs::hard_link(archive_filename, &validator_archive_path).unwrap();
    let slot_floor = archive_slot;

    // Start up a new node from a snapshot
    let validator_stake = 5;

    let (cluster_nodes, _) = discover_cluster(&cluster.entry_point_info.gossip, 1).unwrap();
    let mut trusted_validators = HashSet::new();
    trusted_validators.insert(cluster_nodes[0].id);
    validator_snapshot_test_config
        .validator_config
        .trusted_validators = Some(trusted_validators);

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
    let num_account_paths = 1;
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

        wait_for_next_snapshot(&cluster, &snapshot_package_output_path);

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

fn wait_for_next_snapshot(
    cluster: &LocalCluster,
    snapshot_package_output_path: &Path,
) -> (PathBuf, (Slot, Hash)) {
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
        "Waiting for snapshot archive to be generated with slot > {}",
        last_slot
    );
    loop {
        if let Some((filename, (slot, hash, _))) =
            snapshot_utils::get_highest_snapshot_archive_path(snapshot_package_output_path)
        {
            trace!("snapshot for slot {} exists", slot);
            if slot >= last_slot {
                return (filename, (slot, hash));
            }
            trace!("snapshot slot {} < last_slot {}", slot, last_slot);
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
        compression: CompressionType::Bzip2,
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

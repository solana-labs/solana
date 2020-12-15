use assert_matches::assert_matches;
use crossbeam_channel::{unbounded, Receiver};
use gag::BufferRedirect;
use log::*;
use serial_test_derive::serial;
use solana_client::{
    pubsub_client::PubsubClient,
    rpc_client::RpcClient,
    rpc_config::{RpcProgramAccountsConfig, RpcSignatureSubscribeConfig},
    rpc_response::RpcSignatureResult,
    thin_client::{create_client, ThinClient},
};
use solana_core::{
    broadcast_stage::BroadcastStageType,
    cluster_info::VALIDATOR_PORT_RANGE,
    consensus::{Tower, SWITCH_FORK_THRESHOLD, VOTE_THRESHOLD_DEPTH},
    gossip_service::discover_cluster,
    optimistic_confirmation_verifier::OptimisticConfirmationVerifier,
    validator::ValidatorConfig,
};
use solana_download_utils::download_snapshot;
use solana_ledger::{
    ancestor_iterator::AncestorIterator,
    blockstore::{Blockstore, PurgeType},
    blockstore_db::AccessType,
    leader_schedule::FixedSchedule,
    leader_schedule::LeaderSchedule,
};
use solana_local_cluster::{
    cluster::Cluster,
    cluster_tests,
    local_cluster::{ClusterConfig, LocalCluster},
};
use solana_runtime::{
    bank_forks::{CompressionType, SnapshotConfig},
    snapshot_utils,
};
use solana_sdk::{
    account::Account,
    client::{AsyncClient, SyncClient},
    clock::{self, Slot},
    commitment_config::CommitmentConfig,
    epoch_schedule::MINIMUM_SLOTS_PER_EPOCH,
    genesis_config::ClusterType,
    hash::Hash,
    poh_config::PohConfig,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program, system_transaction,
};
use solana_vote_program::vote_state::MAX_LOCKOUT_HISTORY;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    iter,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    thread::{sleep, Builder, JoinHandle},
    time::Duration,
};
use tempfile::TempDir;

#[test]
#[serial]
fn test_ledger_cleanup_service() {
    solana_logger::setup();
    error!("test_ledger_cleanup_service");
    let num_nodes = 3;
    let validator_config = ValidatorConfig {
        max_ledger_shreds: Some(100),
        ..ValidatorConfig::default()
    };
    let mut config = ClusterConfig {
        cluster_lamports: 10_000,
        poh_config: PohConfig::new_sleep(Duration::from_millis(50)),
        node_stakes: vec![100; num_nodes],
        validator_configs: vec![validator_config; num_nodes],
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config);
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
    for info in cluster.validators.values() {
        let mut slots = 0;
        let blockstore = Blockstore::open(&info.info.ledger_path).unwrap();
        blockstore
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|_| slots += 1);
        // with 3 nodes up to 3 slots can be in progress and not complete so max slots in blockstore should be up to 103
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
#[serial]
fn test_local_cluster_signature_subscribe() {
    solana_logger::setup();
    let num_nodes = 2;
    let cluster = LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100);
    let nodes = cluster.get_node_pubkeys();

    // Get non leader
    let non_bootstrap_id = nodes
        .into_iter()
        .find(|id| *id != cluster.entry_point_info.id)
        .unwrap();
    let non_bootstrap_info = cluster.get_contact_info(&non_bootstrap_id).unwrap();

    let tx_client = create_client(
        non_bootstrap_info.client_facing_addr(),
        VALIDATOR_PORT_RANGE,
    );
    let (blockhash, _fee_calculator, _last_valid_slot) = tx_client
        .get_recent_blockhash_with_commitment(CommitmentConfig::recent())
        .unwrap();

    let mut transaction = system_transaction::transfer(
        &cluster.funding_keypair,
        &solana_sdk::pubkey::new_rand(),
        10,
        blockhash,
    );

    let (mut sig_subscribe_client, receiver) = PubsubClient::signature_subscribe(
        &format!("ws://{}", &non_bootstrap_info.rpc_pubsub.to_string()),
        &transaction.signatures[0],
        Some(RpcSignatureSubscribeConfig {
            commitment: Some(CommitmentConfig::recent()),
            enable_received_notification: Some(true),
        }),
    )
    .unwrap();

    tx_client
        .retry_transfer(&cluster.funding_keypair, &mut transaction, 5)
        .unwrap();

    let mut got_received_notification = false;
    loop {
        let responses: Vec<_> = receiver.try_iter().collect();
        let mut should_break = false;
        for response in responses {
            match response.value {
                RpcSignatureResult::ProcessedSignature(_) => {
                    should_break = true;
                    break;
                }
                RpcSignatureResult::ReceivedSignature(_) => {
                    got_received_notification = true;
                }
            }
        }

        if should_break {
            break;
        }
        sleep(Duration::from_millis(100));
    }

    // If we don't drop the cluster, the blocking web socket service
    // won't return, and the `sig_subscribe_client` won't shut down
    drop(cluster);
    sig_subscribe_client.shutdown().unwrap();
    assert!(got_received_notification);
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

    let mut config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100; num_nodes],
        validator_configs: vec![validator_config; num_nodes],
        ..ClusterConfig::default()
    };
    let local = LocalCluster::new(&mut config);
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
    let mut config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100; 4],
        validator_configs: vec![validator_config; num_nodes],
        ..ClusterConfig::default()
    };
    let local = LocalCluster::new(&mut config);
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
#[allow(clippy::cognitive_complexity)]
fn run_cluster_partition<E, F>(
    partitions: &[&[usize]],
    leader_schedule: Option<(LeaderSchedule, Vec<Arc<Keypair>>)>,
    on_partition_start: E,
    on_partition_resolved: F,
    additional_accounts: Vec<(Pubkey, Account)>,
) where
    E: FnOnce(&mut LocalCluster),
    F: FnOnce(&mut LocalCluster),
{
    solana_logger::setup();
    info!("PARTITION_TEST!");
    let num_nodes = partitions.len();
    let node_stakes: Vec<_> = partitions
        .iter()
        .flat_map(|p| p.iter().map(|stake_weight| 100 * *stake_weight as u64))
        .collect();
    assert_eq!(node_stakes.len(), num_nodes);
    let cluster_lamports = node_stakes.iter().sum::<u64>() * 2;
    let enable_partition = Arc::new(AtomicBool::new(true));
    let mut validator_config = ValidatorConfig {
        enable_partition: Some(enable_partition.clone()),
        ..ValidatorConfig::default()
    };

    // Returns:
    // 1) The keys for the validators
    // 2) The amount of time it would take to iterate through one full iteration of the given
    // leader schedule
    let (validator_keys, leader_schedule_time): (Vec<_>, u64) = {
        if let Some((leader_schedule, validator_keys)) = leader_schedule {
            assert_eq!(validator_keys.len(), num_nodes);
            let num_slots_per_rotation = leader_schedule.num_slots() as u64;
            let fixed_schedule = FixedSchedule {
                start_epoch: 0,
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

    let slots_per_epoch = 2048;
    let mut config = ClusterConfig {
        cluster_lamports,
        node_stakes,
        validator_configs: vec![validator_config; num_nodes],
        validator_keys: Some(
            validator_keys
                .into_iter()
                .zip(iter::repeat_with(|| true))
                .collect(),
        ),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        additional_accounts,
        ..ClusterConfig::default()
    };

    info!(
        "PARTITION_TEST starting cluster with {:?} partitions slots_per_epoch: {}",
        partitions, config.slots_per_epoch,
    );
    let mut cluster = LocalCluster::new(&mut config);

    info!("PARTITION_TEST spend_and_verify_all_nodes(), ensure all nodes are caught up");
    cluster_tests::spend_and_verify_all_nodes(
        &cluster.entry_point_info,
        &cluster.funding_keypair,
        num_nodes,
        HashSet::new(),
    );

    let cluster_nodes = discover_cluster(&cluster.entry_point_info.gossip, num_nodes).unwrap();

    // Check epochs have correct number of slots
    info!("PARTITION_TEST sleeping until partition starting condition",);
    for node in &cluster_nodes {
        let node_client = RpcClient::new_socket(node.rpc);
        let epoch_info = node_client.get_epoch_info().unwrap();
        info!("slots_per_epoch: {:?}", epoch_info);
        assert_eq!(epoch_info.slots_in_epoch, slots_per_epoch);
    }

    info!("PARTITION_TEST start partition");
    enable_partition.store(false, Ordering::Relaxed);
    on_partition_start(&mut cluster);

    sleep(Duration::from_millis(leader_schedule_time));

    info!("PARTITION_TEST remove partition");
    enable_partition.store(true, Ordering::Relaxed);

    // Give partitions time to propagate their blocks from during the partition
    // after the partition resolves
    let timeout = 10_000;
    let propagation_time = leader_schedule_time;
    info!(
        "PARTITION_TEST resolving partition. sleeping {} ms",
        timeout
    );
    sleep(Duration::from_millis(timeout));
    info!(
        "PARTITION_TEST waiting for blocks to propagate after partition {}ms",
        propagation_time
    );
    sleep(Duration::from_millis(propagation_time));
    info!("PARTITION_TEST resuming normal operation");
    on_partition_resolved(&mut cluster);
}

#[allow(unused_attributes)]
#[ignore]
#[test]
#[serial]
fn test_cluster_partition_1_2() {
    let empty = |_: &mut LocalCluster| {};
    let on_partition_resolved = |cluster: &mut LocalCluster| {
        cluster.check_for_new_roots(16, &"PARTITION_TEST");
    };
    run_cluster_partition(&[&[1], &[1, 1]], None, empty, on_partition_resolved, vec![])
}

#[allow(unused_attributes)]
#[ignore]
#[test]
#[serial]
fn test_cluster_partition_1_1() {
    let empty = |_: &mut LocalCluster| {};
    let on_partition_resolved = |cluster: &mut LocalCluster| {
        cluster.check_for_new_roots(16, &"PARTITION_TEST");
    };
    run_cluster_partition(&[&[1], &[1]], None, empty, on_partition_resolved, vec![])
}

#[test]
#[serial]
fn test_cluster_partition_1_1_1() {
    let empty = |_: &mut LocalCluster| {};
    let on_partition_resolved = |cluster: &mut LocalCluster| {
        cluster.check_for_new_roots(16, &"PARTITION_TEST");
    };
    run_cluster_partition(
        &[&[1], &[1], &[1]],
        None,
        empty,
        on_partition_resolved,
        vec![],
    )
}

fn create_custom_leader_schedule(
    num_validators: usize,
    num_slots_per_validator: usize,
) -> (LeaderSchedule, Vec<Arc<Keypair>>) {
    let mut leader_schedule = vec![];
    let validator_keys: Vec<_> = iter::repeat_with(|| Arc::new(Keypair::new()))
        .take(num_validators)
        .collect();
    for (i, k) in validator_keys.iter().enumerate() {
        let num_slots = {
            if i == 0 {
                // Set up the leader to have 50% of the slots
                num_slots_per_validator * (num_validators - 1)
            } else {
                num_slots_per_validator
            }
        };
        for _ in 0..num_slots {
            leader_schedule.push(k.pubkey())
        }
    }

    info!("leader_schedule: {}", leader_schedule.len());
    (
        LeaderSchedule::new_from_schedule(leader_schedule),
        validator_keys,
    )
}

#[test]
#[serial]
fn test_kill_heaviest_partition() {
    // This test:
    // 1) Spins up four partitions, the heaviest being the first with more stake
    // 2) Schedules the other validators for sufficient slots in the schedule
    // so that they will still be locked out of voting for the major partition
    // when the partition resolves
    // 3) Kills the most staked partition. Validators are locked out, but should all
    // eventually choose the major partition
    // 4) Check for recovery
    let num_slots_per_validator = 8;
    let partitions: [&[usize]; 4] = [&[11], &[10], &[10], &[10]];
    let (leader_schedule, validator_keys) =
        create_custom_leader_schedule(partitions.len(), num_slots_per_validator);

    let empty = |_: &mut LocalCluster| {};
    let validator_to_kill = validator_keys[0].pubkey();
    let on_partition_resolved = |cluster: &mut LocalCluster| {
        info!("Killing validator with id: {}", validator_to_kill);
        cluster.exit_node(&validator_to_kill);
        cluster.check_for_new_roots(16, &"PARTITION_TEST");
    };
    run_cluster_partition(
        &partitions,
        Some((leader_schedule, validator_keys)),
        empty,
        on_partition_resolved,
        vec![],
    )
}

#[allow(clippy::assertions_on_constants)]
fn run_kill_partition_switch_threshold<F>(
    failures_stake: u64,
    alive_stake_1: u64,
    alive_stake_2: u64,
    on_partition_resolved: F,
) where
    F: Fn(&mut LocalCluster),
{
    // Needs to be at least 1/3 or there will be no overlap
    // with the confirmation supermajority 2/3
    assert!(SWITCH_FORK_THRESHOLD >= 1f64 / 3f64);
    info!(
        "stakes: {} {} {}",
        failures_stake, alive_stake_1, alive_stake_2
    );

    // This test:
    // 1) Spins up three partitions
    // 2) Kills the first partition with the stake `failures_stake`
    // 5) runs `on_partition_resolved`
    let num_slots_per_validator = 8;
    let partitions: [&[usize]; 3] = [
        &[(failures_stake as usize)],
        &[(alive_stake_1 as usize)],
        &[(alive_stake_2 as usize)],
    ];
    let (leader_schedule, validator_keys) =
        create_custom_leader_schedule(partitions.len(), num_slots_per_validator);

    let validator_to_kill = validator_keys[0].pubkey();
    let on_partition_start = |cluster: &mut LocalCluster| {
        info!("Killing validator with id: {}", validator_to_kill);
        cluster.exit_node(&validator_to_kill);
    };
    run_cluster_partition(
        &partitions,
        Some((leader_schedule, validator_keys)),
        on_partition_start,
        on_partition_resolved,
        vec![],
    )
}

#[test]
#[serial]
fn test_kill_partition_switch_threshold_no_progress() {
    let max_switch_threshold_failure_pct = 1.0 - 2.0 * SWITCH_FORK_THRESHOLD;
    let total_stake = 10_000;
    let max_failures_stake = (max_switch_threshold_failure_pct * total_stake as f64) as u64;

    let failures_stake = max_failures_stake;
    let total_alive_stake = total_stake - failures_stake;
    let alive_stake_1 = total_alive_stake / 2;
    let alive_stake_2 = total_alive_stake - alive_stake_1;

    // Check that no new roots were set 400 slots after partition resolves (gives time
    // for lockouts built during partition to resolve and gives validators an opportunity
    // to try and switch forks)
    let on_partition_resolved = |cluster: &mut LocalCluster| {
        cluster.check_no_new_roots(400, &"PARTITION_TEST");
    };

    // This kills `max_failures_stake`, so no progress should be made
    run_kill_partition_switch_threshold(
        failures_stake,
        alive_stake_1,
        alive_stake_2,
        on_partition_resolved,
    );
}

#[test]
#[serial]
fn test_kill_partition_switch_threshold_progress() {
    let max_switch_threshold_failure_pct = 1.0 - 2.0 * SWITCH_FORK_THRESHOLD;
    let total_stake = 10_000;

    // Kill `< max_failures_stake` of the validators
    let max_failures_stake = (max_switch_threshold_failure_pct * total_stake as f64) as u64;
    let failures_stake = max_failures_stake - 1;
    let total_alive_stake = total_stake - failures_stake;

    // Partition the remaining alive validators, should still make progress
    // once the partition resolves
    let alive_stake_1 = total_alive_stake / 2;
    let alive_stake_2 = total_alive_stake - alive_stake_1;
    let bigger = std::cmp::max(alive_stake_1, alive_stake_2);
    let smaller = std::cmp::min(alive_stake_1, alive_stake_2);

    // At least one of the forks must have > SWITCH_FORK_THRESHOLD in order
    // to guarantee switching proofs can be created. Make sure the other fork
    // is <= SWITCH_FORK_THRESHOLD to make sure progress can be made. Caches
    // bugs such as liveness issues bank-weighted fork choice, which may stall
    // because the fork with less stake could have more weight, but other fork would:
    // 1) Not be able to generate a switching proof
    // 2) Other more staked fork stops voting, so doesn't catch up in bank weight.
    assert!(
        bigger as f64 / total_stake as f64 > SWITCH_FORK_THRESHOLD
            && smaller as f64 / total_stake as f64 <= SWITCH_FORK_THRESHOLD
    );

    let on_partition_resolved = |cluster: &mut LocalCluster| {
        cluster.check_for_new_roots(16, &"PARTITION_TEST");
    };
    run_kill_partition_switch_threshold(
        failures_stake,
        alive_stake_1,
        alive_stake_2,
        on_partition_resolved,
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

    validator_config.rpc_config.enable_validator_exit = true;
    let mut cluster = LocalCluster::new(&mut ClusterConfig {
        node_stakes: vec![999_990, 3],
        cluster_lamports: 1_000_000,
        validator_configs: vec![validator_config; 2],
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
    let mut config = ClusterConfig {
        node_stakes: vec![999_990, 3],
        cluster_lamports: 2_000_000,
        validator_configs: vec![ValidatorConfig::default(); 2],
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config);

    let cluster_nodes = discover_cluster(&cluster.entry_point_info.gossip, 2).unwrap();
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
    let slots_per_epoch = MINIMUM_SLOTS_PER_EPOCH * 2;
    let ticks_per_slot = 16;
    let validator_config = ValidatorConfig::default();
    let mut cluster = LocalCluster::new(&mut ClusterConfig {
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
    let mut config = ClusterConfig {
        node_stakes: vec![100; 1],
        cluster_lamports: 1_000,
        num_listeners: 3,
        validator_configs: vec![ValidatorConfig::default(); 1],
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config);
    let cluster_nodes = discover_cluster(&cluster.entry_point_info.gossip, 4).unwrap();
    assert_eq!(cluster_nodes.len(), 4);
}

#[test]
#[serial]
fn test_mainnet_beta_cluster_type() {
    solana_logger::setup();

    let mut config = ClusterConfig {
        cluster_type: ClusterType::MainnetBeta,
        node_stakes: vec![100; 1],
        cluster_lamports: 1_000,
        validator_configs: vec![ValidatorConfig::default(); 1],
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config);
    let cluster_nodes = discover_cluster(&cluster.entry_point_info.gossip, 1).unwrap();
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
        &solana_sdk::bpf_loader_deprecated::id(),
        &solana_sdk::bpf_loader::id(),
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
        &solana_sdk::bpf_loader_upgradeable::id(),
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
        let (blockhash, _fee_calculator, _last_valid_slot) = client
            .get_recent_blockhash_with_commitment(CommitmentConfig::recent())
            .unwrap();
        client
            .async_transfer(
                1,
                &frozen_account,
                &solana_sdk::pubkey::new_rand(),
                blockhash,
            )
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

    let mut config = ClusterConfig {
        validator_keys: Some(vec![(validator_identity.clone(), true)]),
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
    generate_frozen_account_panic(LocalCluster::new(&mut config), validator_identity);
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

    let mut config = ClusterConfig {
        validator_keys: Some(vec![(validator_identity.clone(), true)]),
        node_stakes: vec![100; 1],
        cluster_lamports: 1_000,
        validator_configs: vec![snapshot_test_config.validator_config.clone()],
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config);

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
    let mut config = ClusterConfig {
        node_stakes: vec![validator_stake],
        cluster_lamports: 100_000,
        validator_configs: vec![leader_snapshot_test_config.validator_config],
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&mut config);

    sleep(Duration::from_millis(5000));
    let cluster_nodes = discover_cluster(&cluster.entry_point_info.gossip, 1).unwrap();
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
        None,
    );
    let num_nodes = 2;
    assert_eq!(
        discover_cluster(&cluster.entry_point_info.gossip, num_nodes)
            .unwrap()
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
                if nodes.len() < 2 {
                    encountered_error = true;
                    break;
                }
                info!("checking cluster for fewer nodes.. {:?}", nodes.len());
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
    let mut config = ClusterConfig {
        node_stakes: vec![stake],
        cluster_lamports: 1_000_000,
        validator_configs: vec![leader_snapshot_test_config.validator_config.clone()],
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&mut config);

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
        false,
    )
    .unwrap();

    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        stake,
        Arc::new(Keypair::new()),
        None,
    );
}

#[allow(unused_attributes)]
#[test]
#[serial]
fn test_snapshot_restart_tower() {
    solana_logger::setup();
    // First set up the cluster with 2 nodes
    let snapshot_interval_slots = 10;
    let num_account_paths = 2;

    let leader_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
    let validator_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);

    let mut config = ClusterConfig {
        node_stakes: vec![10000, 10],
        cluster_lamports: 100_000,
        validator_configs: vec![
            leader_snapshot_test_config.validator_config.clone(),
            validator_snapshot_test_config.validator_config.clone(),
        ],
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&mut config);

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

    let mut config = ClusterConfig {
        node_stakes: vec![10000],
        cluster_lamports: 100_000,
        validator_configs: vec![leader_snapshot_test_config.validator_config.clone()],
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&mut config);

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

    let cluster_nodes = discover_cluster(&cluster.entry_point_info.gossip, 1).unwrap();
    let mut trusted_validators = HashSet::new();
    trusted_validators.insert(cluster_nodes[0].id);
    validator_snapshot_test_config
        .validator_config
        .trusted_validators = Some(trusted_validators);

    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        validator_stake,
        Arc::new(Keypair::new()),
        None,
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

    let mut config = ClusterConfig {
        node_stakes: vec![10000],
        cluster_lamports: 100_000,
        validator_configs: vec![snapshot_test_config.validator_config.clone()],
        ..ClusterConfig::default()
    };

    // Create and reboot the node from snapshot `num_runs` times
    let num_runs = 3;
    let mut expected_balances = HashMap::new();
    let mut cluster = LocalCluster::new(&mut config);
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
    let num_nodes = 2;
    let error_validator_config = ValidatorConfig {
        broadcast_stage_type: faulty_node_type,
        ..ValidatorConfig::default()
    };
    let mut validator_configs = Vec::with_capacity(num_nodes - 1);
    validator_configs.resize_with(num_nodes - 1, ValidatorConfig::default);

    // Push a faulty_bootstrap = vec![error_validator_config];
    validator_configs.insert(0, error_validator_config);
    let node_stakes = vec![300, 100];
    assert_eq!(node_stakes.len(), num_nodes);
    let mut cluster_config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes,
        validator_configs,
        slots_per_epoch: MINIMUM_SLOTS_PER_EPOCH * 2,
        stakers_slot_offset: MINIMUM_SLOTS_PER_EPOCH * 2,
        ..ClusterConfig::default()
    };

    let cluster = LocalCluster::new(&mut cluster_config);

    // Check for new roots
    cluster.check_for_new_roots(16, &"test_faulty_node");
}

#[test]
fn test_wait_for_max_stake() {
    solana_logger::setup();
    let mut validator_config = ValidatorConfig::default();
    validator_config.rpc_config.enable_validator_exit = true;
    let mut config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100; 4],
        validator_configs: vec![validator_config; 4],
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config);
    let client = RpcClient::new_socket(cluster.entry_point_info.rpc);

    assert!(client
        .wait_for_max_stake(CommitmentConfig::default(), 33.0f32)
        .is_ok());
    assert!(client.get_slot().unwrap() > 10);
}

#[test]
// Test that when a leader is leader for banks B_i..B_{i+n}, and B_i is not
// votable, then B_{i+1} still chains to B_i
fn test_no_voting() {
    solana_logger::setup();
    let mut validator_config = ValidatorConfig::default();
    validator_config.rpc_config.enable_validator_exit = true;
    validator_config.voting_disabled = true;
    let mut config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100],
        validator_configs: vec![validator_config],
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config);
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
#[serial]
fn test_optimistic_confirmation_violation_detection() {
    solana_logger::setup();
    let buf = std::env::var("OPTIMISTIC_CONF_TEST_DUMP_LOG")
        .err()
        .map(|_| BufferRedirect::stderr().unwrap());
    // First set up the cluster with 2 nodes
    let slots_per_epoch = 2048;
    let node_stakes = vec![51, 50];
    let validator_keys: Vec<_> = vec![
        "4qhhXNTbKD1a5vxDDLZcHKj7ELNeiivtUBxn3wUK1F5VRsQVP89VUhfXqSfgiFB14GfuBgtrQ96n9NvWQADVkcCg",
        "3kHBzVwie5vTEaY6nFCPeFT8qDpoXzn7dCEioGRNBTnUDpvwnG85w8Wq63gVWpVTP8k2a8cgcWRjSXyUkEygpXWS",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .take(node_stakes.len())
    .collect();
    let mut config = ClusterConfig {
        cluster_lamports: 100_000,
        node_stakes: node_stakes.clone(),
        validator_configs: vec![ValidatorConfig::default(); node_stakes.len()],
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config);
    let entry_point_id = cluster.entry_point_info.id;
    // Let the nodes run for a while. Wait for validators to vote on slot `S`
    // so that the vote on `S-1` is definitely in gossip and optimistic confirmation is
    // detected on slot `S-1` for sure, then stop the heavier of the two
    // validators
    let client = cluster.get_validator_client(&entry_point_id).unwrap();
    let mut prev_voted_slot = 0;
    loop {
        let last_voted_slot = client
            .get_slot_with_commitment(CommitmentConfig::recent())
            .unwrap();
        if last_voted_slot > 50 {
            if prev_voted_slot == 0 {
                prev_voted_slot = last_voted_slot;
            } else {
                break;
            }
        }
        sleep(Duration::from_millis(100));
    }

    let exited_validator_info = cluster.exit_node(&entry_point_id);

    // Mark fork as dead on the heavier validator, this should make the fork effectively
    // dead, even though it was optimistically confirmed. The smaller validator should
    // create and jump over to a new fork
    // Also, remove saved tower to intentionally make the restarted validator to violate the
    // optimistic confirmation
    {
        let blockstore = open_blockstore(&exited_validator_info.info.ledger_path);
        info!(
            "Setting slot: {} on main fork as dead, should cause fork",
            prev_voted_slot
        );
        // marking this voted slot as dead makes the saved tower garbage
        // effectively. That's because its stray last vote becomes stale (= no
        // ancestor in bank forks).
        blockstore.set_dead_slot(prev_voted_slot).unwrap();
    }
    cluster.restart_node(&entry_point_id, exited_validator_info);

    // Wait for a root > prev_voted_slot to be set. Because the root is on a
    // different fork than `prev_voted_slot`, then optimistic confirmation is
    // violated
    let client = cluster.get_validator_client(&entry_point_id).unwrap();
    loop {
        let last_root = client
            .get_slot_with_commitment(CommitmentConfig::max())
            .unwrap();
        if last_root > prev_voted_slot {
            break;
        }
        sleep(Duration::from_millis(100));
    }

    // Make sure validator still makes progress
    cluster_tests::check_for_new_roots(
        16,
        &[cluster.get_contact_info(&entry_point_id).unwrap().clone()],
        "test_optimistic_confirmation_violation",
    );

    // Check to see that validator detected optimistic confirmation for
    // `prev_voted_slot` failed
    let expected_log =
        OptimisticConfirmationVerifier::format_optimistic_confirmd_slot_violation_log(
            prev_voted_slot,
        );
    if let Some(mut buf) = buf {
        let mut output = String::new();
        buf.read_to_string(&mut output).unwrap();
        assert!(output.contains(&expected_log));
    } else {
        panic!("dumped log and disaled testing");
    }
}

#[test]
#[serial]
fn test_validator_saves_tower() {
    solana_logger::setup();

    let validator_config = ValidatorConfig {
        require_tower: true,
        ..ValidatorConfig::default()
    };
    let validator_identity_keypair = Arc::new(Keypair::new());
    let validator_id = validator_identity_keypair.pubkey();
    let mut config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100],
        validator_configs: vec![validator_config],
        validator_keys: Some(vec![(validator_identity_keypair.clone(), true)]),
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config);

    let validator_client = cluster.get_validator_client(&validator_id).unwrap();

    let ledger_path = cluster
        .validators
        .get(&validator_id)
        .unwrap()
        .info
        .ledger_path
        .clone();

    // Wait for some votes to be generated
    let mut last_replayed_root;
    loop {
        if let Ok(slot) = validator_client.get_slot_with_commitment(CommitmentConfig::recent()) {
            trace!("current slot: {}", slot);
            if slot > 2 {
                // this will be the root next time a validator starts
                last_replayed_root = slot;
                break;
            }
        }
        sleep(Duration::from_millis(10));
    }

    // Stop validator and check saved tower
    let validator_info = cluster.exit_node(&validator_id);
    let tower1 = Tower::restore(&ledger_path, &validator_id).unwrap();
    trace!("tower1: {:?}", tower1);
    assert_eq!(tower1.root(), 0);

    // Restart the validator and wait for a new root
    cluster.restart_node(&validator_id, validator_info);
    let validator_client = cluster.get_validator_client(&validator_id).unwrap();

    // Wait for the first root
    loop {
        if let Ok(root) = validator_client.get_slot_with_commitment(CommitmentConfig::root()) {
            trace!("current root: {}", root);
            if root > last_replayed_root + 1 {
                last_replayed_root = root;
                break;
            }
        }
        sleep(Duration::from_millis(50));
    }

    // Stop validator, and check saved tower
    let recent_slot = validator_client
        .get_slot_with_commitment(CommitmentConfig::recent())
        .unwrap();
    let validator_info = cluster.exit_node(&validator_id);
    let tower2 = Tower::restore(&ledger_path, &validator_id).unwrap();
    trace!("tower2: {:?}", tower2);
    assert_eq!(tower2.root(), last_replayed_root);
    last_replayed_root = recent_slot;

    // Rollback saved tower to `tower1` to simulate a validator starting from a newer snapshot
    // without having to wait for that snapshot to be generated in this test
    tower1.save(&validator_identity_keypair).unwrap();

    cluster.restart_node(&validator_id, validator_info);
    let validator_client = cluster.get_validator_client(&validator_id).unwrap();

    // Wait for a new root, demonstrating the validator was able to make progress from the older `tower1`
    loop {
        if let Ok(root) = validator_client.get_slot_with_commitment(CommitmentConfig::root()) {
            trace!(
                "current root: {}, last_replayed_root: {}",
                root,
                last_replayed_root
            );
            if root > last_replayed_root {
                break;
            }
        }
        sleep(Duration::from_millis(50));
    }

    // Check the new root is reflected in the saved tower state
    let mut validator_info = cluster.exit_node(&validator_id);
    let tower3 = Tower::restore(&ledger_path, &validator_id).unwrap();
    trace!("tower3: {:?}", tower3);
    assert!(tower3.root() > last_replayed_root);

    // Remove the tower file entirely and allow the validator to start without a tower.  It will
    // rebuild tower from its vote account contents
    remove_tower(&ledger_path, &validator_id);
    validator_info.config.require_tower = false;

    cluster.restart_node(&validator_id, validator_info);
    let validator_client = cluster.get_validator_client(&validator_id).unwrap();

    // Wait for a couple more slots to pass so another vote occurs
    let current_slot = validator_client
        .get_slot_with_commitment(CommitmentConfig::recent())
        .unwrap();
    loop {
        if let Ok(slot) = validator_client.get_slot_with_commitment(CommitmentConfig::recent()) {
            trace!("current_slot: {}, slot: {}", current_slot, slot);
            if slot > current_slot + 1 {
                break;
            }
        }
        sleep(Duration::from_millis(50));
    }

    cluster.close_preserve_ledgers();

    let tower4 = Tower::restore(&ledger_path, &validator_id).unwrap();
    trace!("tower4: {:?}", tower4);
    // should tower4 advance 1 slot compared to tower3????
    assert_eq!(tower4.root(), tower3.root() + 1);
}

fn open_blockstore(ledger_path: &Path) -> Blockstore {
    Blockstore::open_with_access_type(ledger_path, AccessType::PrimaryOnly, None).unwrap_or_else(
        |e| {
            panic!("Failed to open ledger at {:?}, err: {}", ledger_path, e);
        },
    )
}

fn purge_slots(blockstore: &Blockstore, start_slot: Slot, slot_count: Slot) {
    blockstore.purge_from_next_slots(start_slot, start_slot + slot_count);
    blockstore.purge_slots(start_slot, start_slot + slot_count, PurgeType::Exact);
}

fn restore_tower(ledger_path: &Path, node_pubkey: &Pubkey) -> Option<Tower> {
    let tower = Tower::restore(&ledger_path, &node_pubkey);
    if let Err(tower_err) = tower {
        if tower_err.is_file_missing() {
            return None;
        } else {
            panic!("tower restore failed...: {:?}", tower_err);
        }
    }
    // actually saved tower must have at least one vote.
    Tower::restore(&ledger_path, &node_pubkey).ok()
}

fn last_vote_in_tower(ledger_path: &Path, node_pubkey: &Pubkey) -> Option<Slot> {
    restore_tower(ledger_path, node_pubkey).map(|tower| tower.last_voted_slot().unwrap())
}

fn root_in_tower(ledger_path: &Path, node_pubkey: &Pubkey) -> Option<Slot> {
    restore_tower(ledger_path, node_pubkey).map(|tower| tower.root())
}

fn remove_tower(ledger_path: &Path, node_pubkey: &Pubkey) {
    fs::remove_file(Tower::get_filename(&ledger_path, &node_pubkey)).unwrap();
}

// A bit convoluted test case; but this roughly follows this test theoretical scenario:
//
// Step 1: You have validator A + B with 31% and 36% of the stake:
//
//  S0 -> S1 -> S2 -> S3 (A + B vote, optimistically confirmed)
//
// Step 2: Turn off A + B, and truncate the ledger after slot `S3` (simulate votes not
// landing in next slot).
// Start validator C with 33% of the stake with same ledger, but only up to slot S2.
// Have `C` generate some blocks like:
//
// S0 -> S1 -> S2 -> S4
//
// Step 3: Then restart `A` which had 31% of the stake. With the tower, from `A`'s
// perspective it sees:
//
// S0 -> S1 -> S2 -> S3 (voted)
//             |
//             -> S4 -> S5 (C's vote for S4)
//
// The fork choice rule weights look like:
//
// S0 -> S1 -> S2 (ABC) -> S3
//             |
//             -> S4 (C) -> S5
//
// Step 4:
// Without the persisted tower:
//    `A` would choose to vote on the fork with `S4 -> S5`. This is true even if `A`
//    generates a new fork starting at slot `S3` because `C` has more stake than `A`
//    so `A` will eventually pick the fork `C` is on.
//
//    Furthermore `B`'s vote on `S3` is not observable because there are no
//    descendants of slot `S3`, so that fork will not be chosen over `C`'s fork
//
// With the persisted tower:
//    `A` should not be able to generate a switching proof.
//
fn do_test_optimistic_confirmation_violation_with_or_without_tower(with_tower: bool) {
    solana_logger::setup();

    // First set up the cluster with 4 nodes
    let slots_per_epoch = 2048;
    let node_stakes = vec![31, 36, 33, 0];

    // Each pubkeys are prefixed with A, B, C and D.
    // D is needed to:
    // 1) Propagate A's votes for S2 to validator C after A shuts down so that
    // C can avoid NoPropagatedConfirmation errors and continue to generate blocks
    // 2) Provide gossip discovery for `A` when it restarts because `A` will restart
    // at a different gossip port than the entrypoint saved in C's gossip table
    let validator_keys = vec![
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
        "2saHBBoTkLMmttmPQP8KfBkcCw45S5cwtV3wTdGCscRC8uxdgvHxpHiWXKx4LvJjNJtnNcbSv5NdheokFFqnNDt8",
        "4mx9yoFBeYasDKBGDWCTWGJdWuJCKbgqmuP8bN9umybCh5Jzngw7KQxe99Rf5uzfyzgba1i65rJW4Wqk7Ab5S8ye",
        "3zsEPEDsjfEay7te9XqNjRTCE7vwuT6u4DHzBJC19yp7GS8BuNRMRjnpVrKCBzb3d44kxc4KPGSHkCmk6tEfswCg",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .take(node_stakes.len())
    .collect::<Vec<_>>();
    let validators = validator_keys
        .iter()
        .map(|(kp, _)| kp.pubkey())
        .collect::<Vec<_>>();
    let (validator_a_pubkey, validator_b_pubkey, validator_c_pubkey) =
        (validators[0], validators[1], validators[2]);

    let mut config = ClusterConfig {
        cluster_lamports: 100_000,
        node_stakes: node_stakes.clone(),
        validator_configs: vec![ValidatorConfig::default(); node_stakes.len()],
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config);

    let base_slot = 26; // S2
    let next_slot_on_a = 27; // S3
    let truncated_slots = 100; // just enough to purge all following slots after the S2 and S3

    let val_a_ledger_path = cluster.ledger_path(&validator_a_pubkey);
    let val_b_ledger_path = cluster.ledger_path(&validator_b_pubkey);
    let val_c_ledger_path = cluster.ledger_path(&validator_c_pubkey);

    // Immediately kill validator C
    let validator_c_info = cluster.exit_node(&validator_c_pubkey);

    // Step 1:
    // Let validator A, B, (D) run for a while.
    let (mut validator_a_finished, mut validator_b_finished) = (false, false);
    while !(validator_a_finished && validator_b_finished) {
        sleep(Duration::from_millis(100));

        if let Some(last_vote) = last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey) {
            if !validator_a_finished && last_vote >= next_slot_on_a {
                validator_a_finished = true;
            }
        }
        if let Some(last_vote) = last_vote_in_tower(&val_b_ledger_path, &validator_b_pubkey) {
            if !validator_b_finished && last_vote >= next_slot_on_a {
                validator_b_finished = true;
            }
        }
    }
    // kill them at once after the above loop; otherwise one might stall the other!
    let validator_a_info = cluster.exit_node(&validator_a_pubkey);
    let _validator_b_info = cluster.exit_node(&validator_b_pubkey);

    // Step 2:
    // Stop validator and truncate ledger
    info!("truncate validator C's ledger");
    {
        // first copy from validator A's ledger
        std::fs::remove_dir_all(&validator_c_info.info.ledger_path).unwrap();
        let mut opt = fs_extra::dir::CopyOptions::new();
        opt.copy_inside = true;
        fs_extra::dir::copy(&val_a_ledger_path, &val_c_ledger_path, &opt).unwrap();
        // Remove A's tower in the C's new copied ledger
        remove_tower(&validator_c_info.info.ledger_path, &validator_a_pubkey);

        let blockstore = open_blockstore(&validator_c_info.info.ledger_path);
        purge_slots(&blockstore, base_slot + 1, truncated_slots);
    }
    info!("truncate validator A's ledger");
    {
        let blockstore = open_blockstore(&val_a_ledger_path);
        purge_slots(&blockstore, next_slot_on_a + 1, truncated_slots);
        if !with_tower {
            info!("Removing tower!");
            remove_tower(&val_a_ledger_path, &validator_a_pubkey);

            // Remove next_slot_on_a from ledger to force validator A to select
            // votes_on_c_fork. Otherwise the validator A will immediately vote
            // for 27 on restart, because it hasn't gotten the heavier fork from
            // validator C yet.
            // Then it will be stuck on 27 unable to switch because C doesn't
            // have enough stake to generate a switching proof
            purge_slots(&blockstore, next_slot_on_a, truncated_slots);
        } else {
            info!("Not removing tower!");
        }
    }

    // Step 3:
    // Run validator C only to make it produce and vote on its own fork.
    info!("Restart validator C again!!!");
    let val_c_ledger_path = validator_c_info.info.ledger_path.clone();
    cluster.restart_node(&validator_c_pubkey, validator_c_info);

    let mut votes_on_c_fork = std::collections::BTreeSet::new(); // S4 and S5
    for _ in 0..100 {
        sleep(Duration::from_millis(100));

        if let Some(last_vote) = last_vote_in_tower(&val_c_ledger_path, &validator_c_pubkey) {
            if last_vote != base_slot {
                votes_on_c_fork.insert(last_vote);
                // Collect 4 votes
                if votes_on_c_fork.len() >= 4 {
                    break;
                }
            }
        }
    }
    assert!(!votes_on_c_fork.is_empty());
    info!("collected validator C's votes: {:?}", votes_on_c_fork);

    // Step 4:
    // verify whether there was violation or not
    info!("Restart validator A again!!!");
    cluster.restart_node(&validator_a_pubkey, validator_a_info);

    // monitor for actual votes from validator A
    let mut bad_vote_detected = false;
    let mut a_votes = vec![];
    for _ in 0..100 {
        sleep(Duration::from_millis(100));

        if let Some(last_vote) = last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey) {
            a_votes.push(last_vote);
            let blockstore = Blockstore::open_with_access_type(
                &val_a_ledger_path,
                AccessType::TryPrimaryThenSecondary,
                None,
            )
            .unwrap();
            let mut ancestors = AncestorIterator::new(last_vote, &blockstore);
            if ancestors.any(|a| votes_on_c_fork.contains(&a)) {
                bad_vote_detected = true;
                break;
            }
        }
    }

    info!("Observed A's votes on: {:?}", a_votes);

    // an elaborate way of assert!(with_tower && !bad_vote_detected || ...)
    let expects_optimistic_confirmation_violation = !with_tower;
    if bad_vote_detected != expects_optimistic_confirmation_violation {
        if bad_vote_detected {
            panic!("No violation expected because of persisted tower!");
        } else {
            panic!("Violation expected because of removed persisted tower!");
        }
    } else if bad_vote_detected {
        info!("THIS TEST expected violations. And indeed, there was some, because of removed persisted tower.");
    } else {
        info!("THIS TEST expected no violation. And indeed, there was none, thanks to persisted tower.");
    }
}

enum ClusterMode {
    MasterOnly,
    MasterSlave,
}

fn do_test_future_tower(cluster_mode: ClusterMode) {
    solana_logger::setup();

    // First set up the cluster with 4 nodes
    let slots_per_epoch = 2048;
    let node_stakes = match cluster_mode {
        ClusterMode::MasterOnly => vec![100],
        ClusterMode::MasterSlave => vec![100, 0],
    };

    let validator_keys = vec![
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
        "2saHBBoTkLMmttmPQP8KfBkcCw45S5cwtV3wTdGCscRC8uxdgvHxpHiWXKx4LvJjNJtnNcbSv5NdheokFFqnNDt8",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .take(node_stakes.len())
    .collect::<Vec<_>>();
    let validators = validator_keys
        .iter()
        .map(|(kp, _)| kp.pubkey())
        .collect::<Vec<_>>();
    let validator_a_pubkey = match cluster_mode {
        ClusterMode::MasterOnly => validators[0],
        ClusterMode::MasterSlave => validators[1],
    };

    let mut config = ClusterConfig {
        cluster_lamports: 100_000,
        node_stakes: node_stakes.clone(),
        validator_configs: vec![ValidatorConfig::default(); node_stakes.len()],
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config);

    let val_a_ledger_path = cluster.ledger_path(&validator_a_pubkey);

    loop {
        sleep(Duration::from_millis(100));

        if let Some(root) = root_in_tower(&val_a_ledger_path, &validator_a_pubkey) {
            if root >= 15 {
                break;
            }
        }
    }
    let purged_slot_before_restart = 10;
    let validator_a_info = cluster.exit_node(&validator_a_pubkey);
    {
        // create a warped future tower without mangling the tower itself
        info!(
            "Revert blockstore before slot {} and effectively create a future tower",
            purged_slot_before_restart,
        );
        let blockstore = open_blockstore(&val_a_ledger_path);
        purge_slots(&blockstore, purged_slot_before_restart, 100);
    }

    cluster.restart_node(&validator_a_pubkey, validator_a_info);

    let mut newly_rooted = false;
    let some_root_after_restart = purged_slot_before_restart + 25; // 25 is arbitrary; just wait a bit
    for _ in 0..600 {
        sleep(Duration::from_millis(100));

        if let Some(root) = root_in_tower(&val_a_ledger_path, &validator_a_pubkey) {
            if root >= some_root_after_restart {
                newly_rooted = true;
                break;
            }
        }
    }
    let _validator_a_info = cluster.exit_node(&validator_a_pubkey);
    if newly_rooted {
        // there should be no forks; i.e. monotonically increasing ancestor chain
        let last_vote = last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey).unwrap();
        let blockstore = open_blockstore(&val_a_ledger_path);
        let actual_block_ancestors = AncestorIterator::new_inclusive(last_vote, &blockstore)
            .take_while(|a| *a >= some_root_after_restart)
            .collect::<Vec<_>>();
        let expected_countinuous_no_fork_votes = (some_root_after_restart..=last_vote)
            .rev()
            .collect::<Vec<_>>();
        assert_eq!(actual_block_ancestors, expected_countinuous_no_fork_votes);
        assert!(actual_block_ancestors.len() > MAX_LOCKOUT_HISTORY);
        info!("validator managed to handle future tower!");
    } else {
        panic!("no root detected");
    }
}

#[test]
#[serial]
fn test_future_tower_master_only() {
    do_test_future_tower(ClusterMode::MasterOnly);
}

#[test]
#[serial]
fn test_future_tower_master_slave() {
    do_test_future_tower(ClusterMode::MasterSlave);
}

#[test]
fn test_hard_fork_invalidates_tower() {
    solana_logger::setup();

    // First set up the cluster with 2 nodes
    let slots_per_epoch = 2048;
    let node_stakes = vec![60, 40];

    let validator_keys = vec![
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
        "2saHBBoTkLMmttmPQP8KfBkcCw45S5cwtV3wTdGCscRC8uxdgvHxpHiWXKx4LvJjNJtnNcbSv5NdheokFFqnNDt8",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .take(node_stakes.len())
    .collect::<Vec<_>>();
    let validators = validator_keys
        .iter()
        .map(|(kp, _)| kp.pubkey())
        .collect::<Vec<_>>();

    let validator_a_pubkey = validators[0];
    let validator_b_pubkey = validators[1];

    let mut config = ClusterConfig {
        cluster_lamports: 100_000,
        node_stakes: node_stakes.clone(),
        validator_configs: vec![ValidatorConfig::default(); node_stakes.len()],
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let cluster = std::sync::Arc::new(std::sync::Mutex::new(LocalCluster::new(&mut config)));

    let val_a_ledger_path = cluster.lock().unwrap().ledger_path(&validator_a_pubkey);

    let min_root = 15;
    loop {
        sleep(Duration::from_millis(100));

        if let Some(root) = root_in_tower(&val_a_ledger_path, &validator_a_pubkey) {
            if root >= min_root {
                break;
            }
        }
    }

    let mut validator_a_info = cluster.lock().unwrap().exit_node(&validator_a_pubkey);
    let mut validator_b_info = cluster.lock().unwrap().exit_node(&validator_b_pubkey);

    // setup hard fork at slot < a previously rooted slot!
    let hard_fork_slot = min_root - 5;
    let hard_fork_slots = Some(vec![hard_fork_slot]);
    let mut hard_forks = solana_sdk::hard_forks::HardForks::default();
    hard_forks.register(hard_fork_slot);

    let expected_shred_version = solana_sdk::shred_version::compute_shred_version(
        &cluster.lock().unwrap().genesis_config.hash(),
        Some(&hard_forks),
    );

    validator_a_info.config.new_hard_forks = hard_fork_slots.clone();
    validator_a_info.config.wait_for_supermajority = Some(hard_fork_slot);
    validator_a_info.config.expected_shred_version = Some(expected_shred_version);

    validator_b_info.config.new_hard_forks = hard_fork_slots;
    validator_b_info.config.wait_for_supermajority = Some(hard_fork_slot);
    validator_b_info.config.expected_shred_version = Some(expected_shred_version);

    // restart validator A first
    let cluster_for_a = cluster.clone();
    // Spawn a thread because wait_for_supermajority blocks in Validator::new()!
    let thread = std::thread::spawn(move || {
        let restart_context = cluster_for_a
            .lock()
            .unwrap()
            .create_restart_context(&validator_a_pubkey, &mut validator_a_info);
        let restarted_validator_info =
            LocalCluster::restart_node_with_context(validator_a_info, restart_context);
        cluster_for_a
            .lock()
            .unwrap()
            .add_node(&validator_a_pubkey, restarted_validator_info);
    });

    // test validator A actually to wait for supermajority
    let mut last_vote = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(1000));

        let new_last_vote = last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey).unwrap();
        if let Some(last_vote) = last_vote {
            assert_eq!(last_vote, new_last_vote);
        } else {
            last_vote = Some(new_last_vote);
        }
    }

    // restart validator B normally
    cluster
        .lock()
        .unwrap()
        .restart_node(&validator_b_pubkey, validator_b_info);

    // validator A should now start so join its thread here
    thread.join().unwrap();

    // new slots should be rooted after hard-fork cluster relaunch
    cluster
        .lock()
        .unwrap()
        .check_for_new_roots(16, &"hard fork");
}

#[test]
#[serial]
fn test_no_optimistic_confirmation_violation_with_tower() {
    do_test_optimistic_confirmation_violation_with_or_without_tower(true);
}

#[test]
#[serial]
fn test_optimistic_confirmation_violation_without_tower() {
    do_test_optimistic_confirmation_violation_with_or_without_tower(false);
}

#[test]
#[serial]
fn test_run_test_load_program_accounts_root() {
    run_test_load_program_accounts(CommitmentConfig::root());
}

#[test]
#[serial]
fn test_run_test_load_program_accounts_partition_root() {
    run_test_load_program_accounts_partition(CommitmentConfig::root());
}

fn run_test_load_program_accounts_partition(scan_commitment: CommitmentConfig) {
    let num_slots_per_validator = 8;
    let partitions: [&[usize]; 2] = [&[(1)], &[(1)]];
    let (leader_schedule, validator_keys) =
        create_custom_leader_schedule(partitions.len(), num_slots_per_validator);

    let (update_client_sender, update_client_receiver) = unbounded();
    let (scan_client_sender, scan_client_receiver) = unbounded();
    let exit = Arc::new(AtomicBool::new(false));

    let (t_update, t_scan, additional_accounts) = setup_transfer_scan_threads(
        1000,
        exit.clone(),
        scan_commitment,
        update_client_receiver,
        scan_client_receiver,
    );

    let on_partition_start = |cluster: &mut LocalCluster| {
        let update_client = cluster
            .get_validator_client(&cluster.entry_point_info.id)
            .unwrap();
        update_client_sender.send(update_client).unwrap();
        let scan_client = cluster
            .get_validator_client(&cluster.entry_point_info.id)
            .unwrap();
        scan_client_sender.send(scan_client).unwrap();
    };

    let on_partition_resolved = |cluster: &mut LocalCluster| {
        cluster.check_for_new_roots(20, &"run_test_load_program_accounts_partition");
        exit.store(true, Ordering::Relaxed);
        t_update.join().unwrap();
        t_scan.join().unwrap();
    };

    run_cluster_partition(
        &partitions,
        Some((leader_schedule, validator_keys)),
        on_partition_start,
        on_partition_resolved,
        additional_accounts,
    );
}

fn setup_transfer_scan_threads(
    num_starting_accounts: usize,
    exit: Arc<AtomicBool>,
    scan_commitment: CommitmentConfig,
    update_client_receiver: Receiver<ThinClient>,
    scan_client_receiver: Receiver<ThinClient>,
) -> (JoinHandle<()>, JoinHandle<()>, Vec<(Pubkey, Account)>) {
    let exit_ = exit.clone();
    let starting_keypairs: Arc<Vec<Keypair>> = Arc::new(
        iter::repeat_with(Keypair::new)
            .take(num_starting_accounts)
            .collect(),
    );
    let target_keypairs: Arc<Vec<Keypair>> = Arc::new(
        iter::repeat_with(Keypair::new)
            .take(num_starting_accounts)
            .collect(),
    );
    let starting_accounts: Vec<(Pubkey, Account)> = starting_keypairs
        .iter()
        .map(|k| (k.pubkey(), Account::new(1, 0, &system_program::id())))
        .collect();

    let starting_keypairs_ = starting_keypairs.clone();
    let target_keypairs_ = target_keypairs.clone();
    let t_update = Builder::new()
        .name("update".to_string())
        .spawn(move || {
            let client = update_client_receiver.recv().unwrap();
            loop {
                if exit_.load(Ordering::Relaxed) {
                    return;
                }
                let (blockhash, _fee_calculator, _last_valid_slot) = client
                    .get_recent_blockhash_with_commitment(CommitmentConfig::recent())
                    .unwrap();
                for i in 0..starting_keypairs_.len() {
                    client
                        .async_transfer(
                            1,
                            &starting_keypairs_[i],
                            &target_keypairs_[i].pubkey(),
                            blockhash,
                        )
                        .unwrap();
                }
                for i in 0..starting_keypairs_.len() {
                    client
                        .async_transfer(
                            1,
                            &target_keypairs_[i],
                            &starting_keypairs_[i].pubkey(),
                            blockhash,
                        )
                        .unwrap();
                }
            }
        })
        .unwrap();

    // Scan, the total funds should add up to the original
    let mut scan_commitment_config = RpcProgramAccountsConfig::default();
    scan_commitment_config.account_config.commitment = Some(scan_commitment);
    let tracked_pubkeys: HashSet<Pubkey> = starting_keypairs
        .iter()
        .chain(target_keypairs.iter())
        .map(|k| k.pubkey())
        .collect();
    let expected_total_balance = num_starting_accounts as u64;
    let t_scan = Builder::new()
        .name("scan".to_string())
        .spawn(move || {
            let client = scan_client_receiver.recv().unwrap();
            loop {
                if exit.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(total_scan_balance) = client
                    .get_program_accounts_with_config(
                        &system_program::id(),
                        scan_commitment_config.clone(),
                    )
                    .ok()
                    .map(|result| {
                        result
                            .into_iter()
                            .map(|(key, account)| {
                                if tracked_pubkeys.contains(&key) {
                                    account.lamports
                                } else {
                                    0
                                }
                            })
                            .sum::<u64>()
                    })
                {
                    assert_eq!(total_scan_balance, expected_total_balance);
                }
            }
        })
        .unwrap();

    (t_update, t_scan, starting_accounts)
}

fn run_test_load_program_accounts(scan_commitment: CommitmentConfig) {
    solana_logger::setup();
    // First set up the cluster with 2 nodes
    let slots_per_epoch = 2048;
    let node_stakes = vec![51, 50];
    let validator_keys: Vec<_> = vec![
        "4qhhXNTbKD1a5vxDDLZcHKj7ELNeiivtUBxn3wUK1F5VRsQVP89VUhfXqSfgiFB14GfuBgtrQ96n9NvWQADVkcCg",
        "3kHBzVwie5vTEaY6nFCPeFT8qDpoXzn7dCEioGRNBTnUDpvwnG85w8Wq63gVWpVTP8k2a8cgcWRjSXyUkEygpXWS",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .take(node_stakes.len())
    .collect();

    let num_starting_accounts = 1000;
    let exit = Arc::new(AtomicBool::new(false));
    let (update_client_sender, update_client_receiver) = unbounded();
    let (scan_client_sender, scan_client_receiver) = unbounded();

    // Setup the update/scan threads
    let (t_update, t_scan, starting_accounts) = setup_transfer_scan_threads(
        num_starting_accounts,
        exit.clone(),
        scan_commitment,
        update_client_receiver,
        scan_client_receiver,
    );

    let mut config = ClusterConfig {
        cluster_lamports: 100_000,
        node_stakes: node_stakes.clone(),
        validator_configs: vec![ValidatorConfig::default(); node_stakes.len()],
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        additional_accounts: starting_accounts,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config);

    // Give the threads a client to use for querying the cluster
    let all_pubkeys = cluster.get_node_pubkeys();
    let other_validator_id = all_pubkeys
        .into_iter()
        .find(|x| *x != cluster.entry_point_info.id)
        .unwrap();
    let client = cluster
        .get_validator_client(&cluster.entry_point_info.id)
        .unwrap();
    update_client_sender.send(client).unwrap();
    let scan_client = cluster.get_validator_client(&other_validator_id).unwrap();
    scan_client_sender.send(scan_client).unwrap();

    // Wait for some roots to pass
    cluster.check_for_new_roots(40, &"run_test_load_program_accounts");

    // Exit and ensure no violations of consistency were found
    exit.store(true, Ordering::Relaxed);
    t_update.join().unwrap();
    t_scan.join().unwrap();
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

fn farf_dir() -> PathBuf {
    std::env::var("FARF_DIR")
        .unwrap_or_else(|_| "farf".to_string())
        .into()
}

fn generate_account_paths(num_account_paths: usize) -> (Vec<TempDir>, Vec<PathBuf>) {
    let account_storage_dirs: Vec<TempDir> = (0..num_account_paths)
        .map(|_| tempfile::tempdir_in(farf_dir()).unwrap())
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
    snapshot_interval_slots: u64,
    num_account_paths: usize,
) -> SnapshotValidatorConfig {
    // Create the snapshot config
    let snapshot_dir = tempfile::tempdir_in(farf_dir()).unwrap();
    let snapshot_output_path = tempfile::tempdir_in(farf_dir()).unwrap();
    let snapshot_config = SnapshotConfig {
        snapshot_interval_slots,
        snapshot_package_output_path: PathBuf::from(snapshot_output_path.path()),
        snapshot_path: PathBuf::from(snapshot_dir.path()),
        compression: CompressionType::Bzip2,
        snapshot_version: snapshot_utils::SnapshotVersion::default(),
    };

    // Create the account paths
    let (account_storage_dirs, account_storage_paths) = generate_account_paths(num_account_paths);

    // Create the validator config
    let mut validator_config = ValidatorConfig::default();
    validator_config.rpc_config.enable_validator_exit = true;
    validator_config.snapshot_config = Some(snapshot_config);
    validator_config.account_paths = account_storage_paths;
    validator_config.accounts_hash_interval_slots = snapshot_interval_slots;

    SnapshotValidatorConfig {
        _snapshot_dir: snapshot_dir,
        snapshot_output_path,
        account_storage_dirs,
        validator_config,
    }
}

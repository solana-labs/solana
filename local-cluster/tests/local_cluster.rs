#![allow(clippy::integer_arithmetic)]
use {
    assert_matches::assert_matches,
    crossbeam_channel::{unbounded, Receiver},
    gag::BufferRedirect,
    log::*,
    serial_test::serial,
    solana_client::{
        pubsub_client::PubsubClient,
        rpc_client::RpcClient,
        rpc_config::{RpcProgramAccountsConfig, RpcSignatureSubscribeConfig},
        rpc_response::RpcSignatureResult,
        thin_client::{create_client, ThinClient},
    },
    solana_core::{
        broadcast_stage::{
            broadcast_duplicates_run::BroadcastDuplicatesConfig, BroadcastStageType,
        },
        consensus::{Tower, SWITCH_FORK_THRESHOLD, VOTE_THRESHOLD_DEPTH},
        optimistic_confirmation_verifier::OptimisticConfirmationVerifier,
        replay_stage::DUPLICATE_THRESHOLD,
        tower_storage::{FileTowerStorage, SavedTower, TowerStorage},
        validator::ValidatorConfig,
    },
    solana_download_utils::download_snapshot_archive,
    solana_gossip::{
        cluster_info::VALIDATOR_PORT_RANGE,
        crds::Cursor,
        gossip_service::{self, discover_cluster},
    },
    solana_ledger::{
        ancestor_iterator::AncestorIterator,
        blockstore::{Blockstore, PurgeType},
        blockstore_db::AccessType,
        leader_schedule::{FixedSchedule, LeaderSchedule},
    },
    solana_local_cluster::{
        cluster::{Cluster, ClusterValidatorInfo},
        cluster_tests,
        local_cluster::{ClusterConfig, LocalCluster},
        validator_configs::*,
    },
    solana_runtime::{
        snapshot_archive_info::SnapshotArchiveInfoGetter,
        snapshot_config::SnapshotConfig,
        snapshot_package::SnapshotType,
        snapshot_utils::{self, ArchiveFormat},
    },
    solana_sdk::{
        account::AccountSharedData,
        client::{AsyncClient, SyncClient},
        clock::{self, Slot, DEFAULT_MS_PER_SLOT, DEFAULT_TICKS_PER_SLOT, MAX_PROCESSING_AGE},
        commitment_config::CommitmentConfig,
        epoch_schedule::MINIMUM_SLOTS_PER_EPOCH,
        genesis_config::ClusterType,
        hash::Hash,
        poh_config::PohConfig,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_program, system_transaction,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_vote_program::{vote_state::MAX_LOCKOUT_HISTORY, vote_transaction},
    std::{
        collections::{BTreeSet, HashMap, HashSet},
        fs,
        io::Read,
        iter,
        path::{Path, PathBuf},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{sleep, Builder, JoinHandle},
        time::{Duration, Instant},
    },
    tempfile::TempDir,
};

const RUST_LOG_FILTER: &str =
    "error,solana_core::replay_stage=warn,solana_local_cluster=info,local_cluster=info";

#[test]
#[serial]
fn test_ledger_cleanup_service() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
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
        validator_configs: make_identical_validator_configs(&validator_config, num_nodes),
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
    // 200ms/per * 100 = 20 seconds, so sleep a little longer than that.
    sleep(Duration::from_secs(60));

    cluster_tests::spend_and_verify_all_nodes(
        &cluster.entry_point_info,
        &cluster.funding_keypair,
        num_nodes,
        HashSet::new(),
        SocketAddrSpace::Unspecified,
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
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_spend_and_verify_all_nodes_1");
    let num_nodes = 1;
    let local =
        LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100, SocketAddrSpace::Unspecified);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
        HashSet::new(),
        SocketAddrSpace::Unspecified,
    );
}

#[test]
#[serial]
fn test_spend_and_verify_all_nodes_2() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_spend_and_verify_all_nodes_2");
    let num_nodes = 2;
    let local =
        LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100, SocketAddrSpace::Unspecified);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
        HashSet::new(),
        SocketAddrSpace::Unspecified,
    );
}

#[test]
#[serial]
fn test_spend_and_verify_all_nodes_3() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_spend_and_verify_all_nodes_3");
    let num_nodes = 3;
    let local =
        LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100, SocketAddrSpace::Unspecified);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
        HashSet::new(),
        SocketAddrSpace::Unspecified,
    );
}

#[test]
#[serial]
fn test_local_cluster_signature_subscribe() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let num_nodes = 2;
    let cluster =
        LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100, SocketAddrSpace::Unspecified);
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
    let (blockhash, _) = tx_client
        .get_latest_blockhash_with_commitment(CommitmentConfig::processed())
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
            commitment: Some(CommitmentConfig::processed()),
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
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let num_nodes: usize = std::env::var("NUM_NODES")
        .expect("please set environment variable NUM_NODES")
        .parse()
        .expect("could not parse NUM_NODES as a number");
    let local =
        LocalCluster::new_with_equal_stakes(num_nodes, 10_000, 100, SocketAddrSpace::Unspecified);
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
        HashSet::new(),
        SocketAddrSpace::Unspecified,
    );
}

// Cluster needs a supermajority to remain, so the minimum size for this test is 4
#[test]
#[serial]
fn test_leader_failure_4() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_leader_failure_4");
    let num_nodes = 4;
    let validator_config = ValidatorConfig::default();
    let mut config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100; 4],
        validator_configs: make_identical_validator_configs(&validator_config, num_nodes),
        ..ClusterConfig::default()
    };
    let local = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    cluster_tests::kill_entry_and_spend_and_verify_rest(
        &local.entry_point_info,
        &local
            .validators
            .get(&local.entry_point_info.id)
            .unwrap()
            .config
            .validator_exit,
        &local.funding_keypair,
        num_nodes,
        config.ticks_per_slot * config.poh_config.target_tick_duration.as_millis() as u64,
        SocketAddrSpace::Unspecified,
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
fn run_cluster_partition<C>(
    partitions: &[Vec<usize>],
    leader_schedule: Option<(LeaderSchedule, Vec<Arc<Keypair>>)>,
    mut context: C,
    on_partition_start: impl FnOnce(&mut LocalCluster, &mut C),
    on_before_partition_resolved: impl FnOnce(&mut LocalCluster, &mut C),
    on_partition_resolved: impl FnOnce(&mut LocalCluster, &mut C),
    partition_duration: Option<u64>,
    ticks_per_slot: Option<u64>,
    additional_accounts: Vec<(Pubkey, AccountSharedData)>,
) {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
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
        validator_configs: make_identical_validator_configs(&validator_config, num_nodes),
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
        ticks_per_slot: ticks_per_slot.unwrap_or(DEFAULT_TICKS_PER_SLOT),
        ..ClusterConfig::default()
    };

    info!(
        "PARTITION_TEST starting cluster with {:?} partitions slots_per_epoch: {}",
        partitions, config.slots_per_epoch,
    );
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    info!("PARTITION_TEST spend_and_verify_all_nodes(), ensure all nodes are caught up");
    cluster_tests::spend_and_verify_all_nodes(
        &cluster.entry_point_info,
        &cluster.funding_keypair,
        num_nodes,
        HashSet::new(),
        SocketAddrSpace::Unspecified,
    );

    let cluster_nodes = discover_cluster(
        &cluster.entry_point_info.gossip,
        num_nodes,
        SocketAddrSpace::Unspecified,
    )
    .unwrap();

    // Check epochs have correct number of slots
    info!("PARTITION_TEST sleeping until partition starting condition",);
    for node in &cluster_nodes {
        let node_client = RpcClient::new_socket(node.rpc);
        let epoch_info = node_client.get_epoch_info().unwrap();
        assert_eq!(epoch_info.slots_in_epoch, slots_per_epoch);
    }

    info!("PARTITION_TEST start partition");
    on_partition_start(&mut cluster, &mut context);
    enable_partition.store(false, Ordering::Relaxed);

    sleep(Duration::from_millis(
        partition_duration.unwrap_or(leader_schedule_time),
    ));

    on_before_partition_resolved(&mut cluster, &mut context);
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
    on_partition_resolved(&mut cluster, &mut context);
}

#[allow(unused_attributes)]
#[ignore]
#[test]
#[serial]
fn test_cluster_partition_1_2() {
    let empty = |_: &mut LocalCluster, _: &mut ()| {};
    let on_partition_resolved = |cluster: &mut LocalCluster, _: &mut ()| {
        cluster.check_for_new_roots(16, "PARTITION_TEST", SocketAddrSpace::Unspecified);
    };
    run_cluster_partition(
        &[vec![1], vec![1, 1]],
        None,
        (),
        empty,
        empty,
        on_partition_resolved,
        None,
        None,
        vec![],
    )
}

#[test]
#[serial]
fn test_cluster_partition_1_1() {
    let empty = |_: &mut LocalCluster, _: &mut ()| {};
    let on_partition_resolved = |cluster: &mut LocalCluster, _: &mut ()| {
        cluster.check_for_new_roots(16, "PARTITION_TEST", SocketAddrSpace::Unspecified);
    };
    run_cluster_partition(
        &[vec![1], vec![1]],
        None,
        (),
        empty,
        empty,
        on_partition_resolved,
        None,
        None,
        vec![],
    )
}

#[test]
#[serial]
fn test_cluster_partition_1_1_1() {
    let empty = |_: &mut LocalCluster, _: &mut ()| {};
    let on_partition_resolved = |cluster: &mut LocalCluster, _: &mut ()| {
        cluster.check_for_new_roots(16, "PARTITION_TEST", SocketAddrSpace::Unspecified);
    };
    run_cluster_partition(
        &[vec![1], vec![1], vec![1]],
        None,
        (),
        empty,
        empty,
        on_partition_resolved,
        None,
        None,
        vec![],
    )
}

fn create_custom_leader_schedule(
    validator_num_slots: &[usize],
) -> (LeaderSchedule, Vec<Arc<Keypair>>) {
    let mut leader_schedule = vec![];
    let validator_keys: Vec<_> = iter::repeat_with(|| Arc::new(Keypair::new()))
        .take(validator_num_slots.len())
        .collect();
    for (k, num_slots) in validator_keys.iter().zip(validator_num_slots.iter()) {
        for _ in 0..*num_slots {
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
    let partitions: [Vec<usize>; 4] = [vec![11], vec![10], vec![10], vec![10]];
    let (leader_schedule, validator_keys) = create_custom_leader_schedule(&[
        num_slots_per_validator * (partitions.len() - 1),
        num_slots_per_validator,
        num_slots_per_validator,
        num_slots_per_validator,
    ]);

    let empty = |_: &mut LocalCluster, _: &mut ()| {};
    let validator_to_kill = validator_keys[0].pubkey();
    let on_partition_resolved = |cluster: &mut LocalCluster, _: &mut ()| {
        info!("Killing validator with id: {}", validator_to_kill);
        cluster.exit_node(&validator_to_kill);
        cluster.check_for_new_roots(16, "PARTITION_TEST", SocketAddrSpace::Unspecified);
    };
    run_cluster_partition(
        &partitions,
        Some((leader_schedule, validator_keys)),
        (),
        empty,
        empty,
        on_partition_resolved,
        None,
        None,
        vec![],
    )
}

#[allow(clippy::assertions_on_constants)]
fn run_kill_partition_switch_threshold<C>(
    stakes_to_kill: &[&[(usize, usize)]],
    alive_stakes: &[&[(usize, usize)]],
    partition_duration: Option<u64>,
    ticks_per_slot: Option<u64>,
    partition_context: C,
    on_partition_start: impl Fn(&mut LocalCluster, &[Pubkey], Vec<ClusterValidatorInfo>, &mut C),
    on_before_partition_resolved: impl Fn(&mut LocalCluster, &mut C),
    on_partition_resolved: impl Fn(&mut LocalCluster, &mut C),
) {
    // Needs to be at least 1/3 or there will be no overlap
    // with the confirmation supermajority 2/3
    assert!(SWITCH_FORK_THRESHOLD >= 1f64 / 3f64);
    info!(
        "stakes_to_kill: {:?}, alive_stakes: {:?}",
        stakes_to_kill, alive_stakes
    );

    // This test:
    // 1) Spins up three partitions
    // 2) Kills the first partition with the stake `failures_stake`
    // 5) runs `on_partition_resolved`
    let partitions: Vec<&[(usize, usize)]> = stakes_to_kill
        .iter()
        .cloned()
        .chain(alive_stakes.iter().cloned())
        .collect();

    let stake_partitions: Vec<Vec<usize>> = partitions
        .iter()
        .map(|stakes_and_slots| stakes_and_slots.iter().map(|(stake, _)| *stake).collect())
        .collect();
    let num_slots_per_validator: Vec<usize> = partitions
        .iter()
        .flat_map(|stakes_and_slots| stakes_and_slots.iter().map(|(_, num_slots)| *num_slots))
        .collect();

    let (leader_schedule, validator_keys) = create_custom_leader_schedule(&num_slots_per_validator);

    info!(
        "Validator ids: {:?}",
        validator_keys
            .iter()
            .map(|k| k.pubkey())
            .collect::<Vec<_>>()
    );
    let validator_pubkeys: Vec<Pubkey> = validator_keys.iter().map(|k| k.pubkey()).collect();
    let on_partition_start = |cluster: &mut LocalCluster, partition_context: &mut C| {
        let dead_validator_infos: Vec<ClusterValidatorInfo> = validator_pubkeys
            [0..stakes_to_kill.len()]
            .iter()
            .map(|validator_to_kill| {
                info!("Killing validator with id: {}", validator_to_kill);
                cluster.exit_node(validator_to_kill)
            })
            .collect();
        on_partition_start(
            cluster,
            &validator_pubkeys,
            dead_validator_infos,
            partition_context,
        );
    };
    run_cluster_partition(
        &stake_partitions,
        Some((leader_schedule, validator_keys)),
        partition_context,
        on_partition_start,
        on_before_partition_resolved,
        on_partition_resolved,
        partition_duration,
        ticks_per_slot,
        vec![],
    )
}

fn find_latest_replayed_slot_from_ledger(
    ledger_path: &Path,
    mut latest_slot: Slot,
) -> (Slot, HashSet<Slot>) {
    loop {
        let mut blockstore = open_blockstore(ledger_path);
        // This is kind of a hack because we can't query for new frozen blocks over RPC
        // since the validator is not voting.
        let new_latest_slots: Vec<Slot> = blockstore
            .slot_meta_iterator(latest_slot)
            .unwrap()
            .filter_map(|(s, _)| if s > latest_slot { Some(s) } else { None })
            .collect();

        for new_latest_slot in new_latest_slots {
            latest_slot = new_latest_slot;
            info!("Checking latest_slot {}", latest_slot);
            // Wait for the slot to be fully received by the validator
            let entries;
            loop {
                info!("Waiting for slot {} to be full", latest_slot);
                if blockstore.is_full(latest_slot) {
                    entries = blockstore.get_slot_entries(latest_slot, 0).unwrap();
                    assert!(!entries.is_empty());
                    break;
                } else {
                    sleep(Duration::from_millis(50));
                    blockstore = open_blockstore(ledger_path);
                }
            }
            // Check the slot has been replayed
            let non_tick_entry = entries.into_iter().find(|e| !e.transactions.is_empty());
            if let Some(non_tick_entry) = non_tick_entry {
                // Wait for the slot to be replayed
                loop {
                    info!("Waiting for slot {} to be replayed", latest_slot);
                    if !blockstore
                        .map_transactions_to_statuses(
                            latest_slot,
                            non_tick_entry.transactions.clone().into_iter(),
                        )
                        .unwrap()
                        .is_empty()
                    {
                        return (
                            latest_slot,
                            AncestorIterator::new(latest_slot, &blockstore).collect(),
                        );
                    } else {
                        sleep(Duration::from_millis(50));
                        blockstore = open_blockstore(ledger_path);
                    }
                }
            } else {
                info!(
                    "No transactions in slot {}, can't tell if it was replayed",
                    latest_slot
                );
            }
        }
        sleep(Duration::from_millis(50));
    }
}

#[test]
#[serial]
fn test_switch_threshold_uses_gossip_votes() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let total_stake = 100;

    // Minimum stake needed to generate a switching proof
    let minimum_switch_stake = (SWITCH_FORK_THRESHOLD as f64 * total_stake as f64) as u64;

    // Make the heavier stake insufficient for switching so tha the lighter validator
    // cannot switch without seeing a vote from the dead/failure_stake validator.
    let heavier_stake = minimum_switch_stake;
    let lighter_stake = heavier_stake - 1;
    let failures_stake = total_stake - heavier_stake - lighter_stake;

    let partitions: &[&[(usize, usize)]] = &[
        &[(heavier_stake as usize, 8)],
        &[(lighter_stake as usize, 8)],
    ];

    #[derive(Default)]
    struct PartitionContext {
        heaviest_validator_key: Pubkey,
        lighter_validator_key: Pubkey,
        dead_validator_info: Option<ClusterValidatorInfo>,
    }

    let on_partition_start = |_cluster: &mut LocalCluster,
                              validator_keys: &[Pubkey],
                              mut dead_validator_infos: Vec<ClusterValidatorInfo>,
                              context: &mut PartitionContext| {
        assert_eq!(dead_validator_infos.len(), 1);
        context.dead_validator_info = Some(dead_validator_infos.pop().unwrap());
        // validator_keys[0] is the validator that will be killed, i.e. the validator with
        // stake == `failures_stake`
        context.heaviest_validator_key = validator_keys[1];
        context.lighter_validator_key = validator_keys[2];
    };

    let on_before_partition_resolved = |_: &mut LocalCluster, _: &mut PartitionContext| {};

    // Check that new roots were set after the partition resolves (gives time
    // for lockouts built during partition to resolve and gives validators an opportunity
    // to try and switch forks)
    let on_partition_resolved = |cluster: &mut LocalCluster, context: &mut PartitionContext| {
        let lighter_validator_ledger_path = cluster.ledger_path(&context.lighter_validator_key);
        let heavier_validator_ledger_path = cluster.ledger_path(&context.heaviest_validator_key);

        let (lighter_validator_latest_vote, _) = last_vote_in_tower(
            &lighter_validator_ledger_path,
            &context.lighter_validator_key,
        )
        .unwrap();

        info!(
            "Lighter validator's latest vote is for slot {}",
            lighter_validator_latest_vote
        );

        // Lighter partition should stop voting after detecting the heavier partition and try
        // to switch. Loop until we see a greater vote by the heavier validator than the last
        // vote made by the lighter validator on the lighter fork.
        let mut heavier_validator_latest_vote;
        let mut heavier_validator_latest_vote_hash;
        let heavier_blockstore = open_blockstore(&heavier_validator_ledger_path);
        loop {
            let (sanity_check_lighter_validator_latest_vote, _) = last_vote_in_tower(
                &lighter_validator_ledger_path,
                &context.lighter_validator_key,
            )
            .unwrap();

            // Lighter validator should stop voting, because `on_partition_resolved` is only
            // called after a propagation time where blocks from the other fork should have
            // finished propagating
            assert_eq!(
                sanity_check_lighter_validator_latest_vote,
                lighter_validator_latest_vote
            );

            let (new_heavier_validator_latest_vote, new_heavier_validator_latest_vote_hash) =
                last_vote_in_tower(
                    &heavier_validator_ledger_path,
                    &context.heaviest_validator_key,
                )
                .unwrap();

            heavier_validator_latest_vote = new_heavier_validator_latest_vote;
            heavier_validator_latest_vote_hash = new_heavier_validator_latest_vote_hash;

            // Latest vote for each validator should be on different forks
            assert_ne!(lighter_validator_latest_vote, heavier_validator_latest_vote);
            if heavier_validator_latest_vote > lighter_validator_latest_vote {
                let heavier_ancestors: HashSet<Slot> =
                    AncestorIterator::new(heavier_validator_latest_vote, &heavier_blockstore)
                        .collect();
                assert!(!heavier_ancestors.contains(&lighter_validator_latest_vote));
                break;
            }
        }

        info!("Checking to make sure lighter validator doesn't switch");
        let mut latest_slot = lighter_validator_latest_vote;

        // Number of chances the validator had to switch votes but didn't
        let mut total_voting_opportunities = 0;
        while total_voting_opportunities <= 5 {
            let (new_latest_slot, latest_slot_ancestors) =
                find_latest_replayed_slot_from_ledger(&lighter_validator_ledger_path, latest_slot);
            latest_slot = new_latest_slot;
            // Ensure `latest_slot` is on the other fork
            if latest_slot_ancestors.contains(&heavier_validator_latest_vote) {
                let tower = restore_tower(
                    &lighter_validator_ledger_path,
                    &context.lighter_validator_key,
                )
                .unwrap();
                // Check that there was an opportunity to vote
                if !tower.is_locked_out(latest_slot, &latest_slot_ancestors) {
                    // Ensure the lighter blockstore has not voted again
                    let new_lighter_validator_latest_vote = tower.last_voted_slot().unwrap();
                    assert_eq!(
                        new_lighter_validator_latest_vote,
                        lighter_validator_latest_vote
                    );
                    info!(
                        "Incrementing voting opportunities: {}",
                        total_voting_opportunities
                    );
                    total_voting_opportunities += 1;
                } else {
                    info!(
                        "Tower still locked out, can't vote for slot: {}",
                        latest_slot
                    );
                }
            } else if latest_slot > heavier_validator_latest_vote {
                warn!(
                    "validator is still generating blocks on its own fork, last processed slot: {}",
                    latest_slot
                );
            }
            sleep(Duration::from_millis(50));
        }

        // Make a vote from the killed validator for slot `heavier_validator_latest_vote` in gossip
        info!(
            "Simulate vote for slot: {} from dead validator",
            heavier_validator_latest_vote
        );
        let vote_keypair = &context
            .dead_validator_info
            .as_ref()
            .unwrap()
            .info
            .voting_keypair
            .clone();
        let node_keypair = &context
            .dead_validator_info
            .as_ref()
            .unwrap()
            .info
            .keypair
            .clone();

        cluster_tests::submit_vote_to_cluster_gossip(
            node_keypair,
            vote_keypair,
            heavier_validator_latest_vote,
            heavier_validator_latest_vote_hash,
            // Make the vote transaction with a random blockhash. Thus, the vote only lives in gossip but
            // never makes it into a block
            Hash::new_unique(),
            cluster
                .get_contact_info(&context.heaviest_validator_key)
                .unwrap()
                .gossip,
            &SocketAddrSpace::Unspecified,
        )
        .unwrap();

        loop {
            // Wait for the lighter validator to switch to the heavier fork
            let (new_lighter_validator_latest_vote, _) = last_vote_in_tower(
                &lighter_validator_ledger_path,
                &context.lighter_validator_key,
            )
            .unwrap();

            if new_lighter_validator_latest_vote != lighter_validator_latest_vote {
                info!(
                    "Lighter validator switched forks at slot: {}",
                    new_lighter_validator_latest_vote
                );
                let (heavier_validator_latest_vote, _) = last_vote_in_tower(
                    &heavier_validator_ledger_path,
                    &context.heaviest_validator_key,
                )
                .unwrap();
                let (smaller, larger) =
                    if new_lighter_validator_latest_vote > heavier_validator_latest_vote {
                        (
                            heavier_validator_latest_vote,
                            new_lighter_validator_latest_vote,
                        )
                    } else {
                        (
                            new_lighter_validator_latest_vote,
                            heavier_validator_latest_vote,
                        )
                    };

                // Check the new vote is on the same fork as the heaviest fork
                let heavier_blockstore = open_blockstore(&heavier_validator_ledger_path);
                let larger_slot_ancestors: HashSet<Slot> =
                    AncestorIterator::new(larger, &heavier_blockstore)
                        .chain(std::iter::once(larger))
                        .collect();
                assert!(larger_slot_ancestors.contains(&smaller));
                break;
            } else {
                sleep(Duration::from_millis(50));
            }
        }
    };

    let ticks_per_slot = 8;
    run_kill_partition_switch_threshold(
        &[&[(failures_stake as usize, 0)]],
        partitions,
        // Partition long enough such that the first vote made by validator with
        // `alive_stake_3` won't be ingested due to BlockhashTooOld,
        None,
        Some(ticks_per_slot),
        PartitionContext::default(),
        on_partition_start,
        on_before_partition_resolved,
        on_partition_resolved,
    );
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
    let on_partition_start =
        |_: &mut LocalCluster, _: &[Pubkey], _: Vec<ClusterValidatorInfo>, _: &mut ()| {};
    let on_before_partition_resolved = |_: &mut LocalCluster, _: &mut ()| {};
    let on_partition_resolved = |cluster: &mut LocalCluster, _: &mut ()| {
        cluster.check_no_new_roots(400, "PARTITION_TEST", SocketAddrSpace::Unspecified);
    };

    // This kills `max_failures_stake`, so no progress should be made
    run_kill_partition_switch_threshold(
        &[&[(failures_stake as usize, 16)]],
        &[
            &[(alive_stake_1 as usize, 8)],
            &[(alive_stake_2 as usize, 8)],
        ],
        None,
        None,
        (),
        on_partition_start,
        on_before_partition_resolved,
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

    let on_partition_start =
        |_: &mut LocalCluster, _: &[Pubkey], _: Vec<ClusterValidatorInfo>, _: &mut ()| {};
    let on_before_partition_resolved = |_: &mut LocalCluster, _: &mut ()| {};
    let on_partition_resolved = |cluster: &mut LocalCluster, _: &mut ()| {
        cluster.check_for_new_roots(16, "PARTITION_TEST", SocketAddrSpace::Unspecified);
    };
    run_kill_partition_switch_threshold(
        &[&[(failures_stake as usize, 16)]],
        &[
            &[(alive_stake_1 as usize, 8)],
            &[(alive_stake_2 as usize, 8)],
        ],
        None,
        None,
        (),
        on_partition_start,
        on_before_partition_resolved,
        on_partition_resolved,
    );
}

#[test]
#[serial]
// Steps in this test:
// We want to create a situation like:
/*
      1 (2%, killed and restarted) --- 200 (37%, lighter fork)
    /
    0
    \-------- 4 (38%, heavier fork)
*/
// where the 2% that voted on slot 1 don't see their votes land in a block
// due to blockhash expiration, and thus without resigning their votes with
// a newer blockhash, will deem slot 4 the heavier fork and try to switch to
// slot 4, which doesn't pass the switch threshold. This stalls the network.

// We do this by:
// 1) Creating a partition so all three nodes don't see each other
// 2) Kill the validator with 2%
// 3) Wait for longer than blockhash expiration
// 4) Copy in the lighter fork's blocks up, *only* up to the first slot in the lighter fork
// (not all the blocks on the lighter fork!), call this slot `L`
// 5) Restart the validator with 2% so that he votes on `L`, but the vote doesn't land
// due to blockhash expiration
// 6) Resolve the partition so that the 2% repairs the other fork, and tries to switch,
// stalling the network.

fn test_fork_choice_refresh_old_votes() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let max_switch_threshold_failure_pct = 1.0 - 2.0 * SWITCH_FORK_THRESHOLD;
    let total_stake = 100;
    let max_failures_stake = (max_switch_threshold_failure_pct * total_stake as f64) as u64;

    // 1% less than the failure stake, where the 2% is allocated to a validator that
    // has no leader slots and thus won't be able to vote on its own fork.
    let failures_stake = max_failures_stake;
    let total_alive_stake = total_stake - failures_stake;
    let alive_stake_1 = total_alive_stake / 2 - 1;
    let alive_stake_2 = total_alive_stake - alive_stake_1 - 1;

    // Heavier fork still doesn't have enough stake to switch. Both branches need
    // the vote to land from the validator with `alive_stake_3` to allow the other
    // fork to switch.
    let alive_stake_3 = 2;
    assert!(alive_stake_1 < alive_stake_2);
    assert!(alive_stake_1 + alive_stake_3 > alive_stake_2);

    let partitions: &[&[(usize, usize)]] = &[
        &[(alive_stake_1 as usize, 8)],
        &[(alive_stake_2 as usize, 8)],
        &[(alive_stake_3 as usize, 0)],
    ];

    #[derive(Default)]
    struct PartitionContext {
        alive_stake3_info: Option<ClusterValidatorInfo>,
        smallest_validator_key: Pubkey,
        lighter_fork_validator_key: Pubkey,
        heaviest_validator_key: Pubkey,
    }
    let on_partition_start = |cluster: &mut LocalCluster,
                              validator_keys: &[Pubkey],
                              _: Vec<ClusterValidatorInfo>,
                              context: &mut PartitionContext| {
        // Kill validator with alive_stake_3, second in `partitions` slice
        let smallest_validator_key = &validator_keys[3];
        let info = cluster.exit_node(smallest_validator_key);
        context.alive_stake3_info = Some(info);
        context.smallest_validator_key = *smallest_validator_key;
        // validator_keys[0] is the validator that will be killed, i.e. the validator with
        // stake == `failures_stake`
        context.lighter_fork_validator_key = validator_keys[1];
        // Third in `partitions` slice
        context.heaviest_validator_key = validator_keys[2];
    };

    let ticks_per_slot = 8;
    let on_before_partition_resolved =
        |cluster: &mut LocalCluster, context: &mut PartitionContext| {
            // Equal to ms_per_slot * MAX_PROCESSING_AGE, rounded up
            let sleep_time_ms = ms_for_n_slots(MAX_PROCESSING_AGE as u64, ticks_per_slot);
            info!("Wait for blockhashes to expire, {} ms", sleep_time_ms);

            // Wait for blockhashes to expire
            sleep(Duration::from_millis(sleep_time_ms));

            let smallest_ledger_path = context
                .alive_stake3_info
                .as_ref()
                .unwrap()
                .info
                .ledger_path
                .clone();
            let lighter_fork_ledger_path = cluster.ledger_path(&context.lighter_fork_validator_key);
            let heaviest_ledger_path = cluster.ledger_path(&context.heaviest_validator_key);

            // Get latest votes. We make sure to wait until the vote has landed in
            // blockstore. This is important because if we were the leader for the block there
            // is a possibility of voting before broadcast has inserted in blockstore.
            let lighter_fork_latest_vote = wait_for_last_vote_in_tower_to_land_in_ledger(
                &lighter_fork_ledger_path,
                &context.lighter_fork_validator_key,
            );
            let heaviest_fork_latest_vote = wait_for_last_vote_in_tower_to_land_in_ledger(
                &heaviest_ledger_path,
                &context.heaviest_validator_key,
            );

            // Open ledgers
            let smallest_blockstore = open_blockstore(&smallest_ledger_path);
            let lighter_fork_blockstore = open_blockstore(&lighter_fork_ledger_path);
            let heaviest_blockstore = open_blockstore(&heaviest_ledger_path);

            info!("Opened blockstores");

            // Find the first slot on the smaller fork
            let lighter_ancestors: BTreeSet<Slot> = std::iter::once(lighter_fork_latest_vote)
                .chain(AncestorIterator::new(
                    lighter_fork_latest_vote,
                    &lighter_fork_blockstore,
                ))
                .collect();
            let heavier_ancestors: BTreeSet<Slot> = std::iter::once(heaviest_fork_latest_vote)
                .chain(AncestorIterator::new(
                    heaviest_fork_latest_vote,
                    &heaviest_blockstore,
                ))
                .collect();
            let first_slot_in_lighter_partition = *lighter_ancestors
                .iter()
                .zip(heavier_ancestors.iter())
                .find(|(x, y)| x != y)
                .unwrap()
                .0;

            // Must have been updated in the above loop
            assert!(first_slot_in_lighter_partition != 0);
            info!(
                "First slot in lighter partition is {}",
                first_slot_in_lighter_partition
            );

            // Copy all the blocks from the smaller partition up to `first_slot_in_lighter_partition`
            // into the smallest validator's blockstore
            copy_blocks(
                first_slot_in_lighter_partition,
                &lighter_fork_blockstore,
                &smallest_blockstore,
            );

            // Restart the smallest validator that we killed earlier in `on_partition_start()`
            drop(smallest_blockstore);
            cluster.restart_node(
                &context.smallest_validator_key,
                context.alive_stake3_info.take().unwrap(),
                SocketAddrSpace::Unspecified,
            );

            loop {
                // Wait for node to vote on the first slot on the less heavy fork, so it'll need
                // a switch proof to flip to the other fork.
                // However, this vote won't land because it's using an expired blockhash. The
                // fork structure will look something like this after the vote:
                /*
                     1 (2%, killed and restarted) --- 200 (37%, lighter fork)
                    /
                    0
                    \-------- 4 (38%, heavier fork)
                */
                if let Some((last_vote_slot, _last_vote_hash)) =
                    last_vote_in_tower(&smallest_ledger_path, &context.smallest_validator_key)
                {
                    // Check that the heaviest validator on the other fork doesn't have this slot,
                    // this must mean we voted on a unique slot on this fork
                    if last_vote_slot == first_slot_in_lighter_partition {
                        info!(
                            "Saw vote on first slot in lighter partition {}",
                            first_slot_in_lighter_partition
                        );
                        break;
                    } else {
                        info!(
                            "Haven't seen vote on first slot in lighter partition, latest vote is: {}",
                            last_vote_slot
                        );
                    }
                }

                sleep(Duration::from_millis(20));
            }

            // Now resolve partition, allow validator to see the fork with the heavier validator,
            // but the fork it's currently on is the heaviest, if only its own vote landed!
        };

    // Check that new roots were set after the partition resolves (gives time
    // for lockouts built during partition to resolve and gives validators an opportunity
    // to try and switch forks)
    let on_partition_resolved = |cluster: &mut LocalCluster, _: &mut PartitionContext| {
        cluster.check_for_new_roots(16, "PARTITION_TEST", SocketAddrSpace::Unspecified);
    };

    run_kill_partition_switch_threshold(
        &[&[(failures_stake as usize - 1, 16)]],
        partitions,
        // Partition long enough such that the first vote made by validator with
        // `alive_stake_3` won't be ingested due to BlockhashTooOld,
        None,
        Some(ticks_per_slot),
        PartitionContext::default(),
        on_partition_start,
        on_before_partition_resolved,
        on_partition_resolved,
    );
}

#[test]
#[serial]
fn test_two_unbalanced_stakes() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_two_unbalanced_stakes");
    let validator_config = ValidatorConfig::default();
    let num_ticks_per_second = 100;
    let num_ticks_per_slot = 10;
    let num_slots_per_epoch = MINIMUM_SLOTS_PER_EPOCH as u64;

    let mut cluster = LocalCluster::new(
        &mut ClusterConfig {
            node_stakes: vec![999_990, 3],
            cluster_lamports: 1_000_000,
            validator_configs: make_identical_validator_configs(&validator_config, 2),
            ticks_per_slot: num_ticks_per_slot,
            slots_per_epoch: num_slots_per_epoch,
            stakers_slot_offset: num_slots_per_epoch,
            poh_config: PohConfig::new_sleep(Duration::from_millis(1000 / num_ticks_per_second)),
            ..ClusterConfig::default()
        },
        SocketAddrSpace::Unspecified,
    );

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
        validator_configs: make_identical_validator_configs(&ValidatorConfig::default(), 2),
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let cluster_nodes = discover_cluster(
        &cluster.entry_point_info.gossip,
        2,
        SocketAddrSpace::Unspecified,
    )
    .unwrap();
    assert!(cluster_nodes.len() >= 2);

    let leader_pubkey = cluster.entry_point_info.id;

    let validator_info = cluster_nodes
        .iter()
        .find(|c| c.id != leader_pubkey)
        .unwrap();

    // Confirm that transactions were forwarded to and processed by the leader.
    cluster_tests::send_many_transactions(validator_info, &cluster.funding_keypair, 10, 20);
}

#[test]
#[serial]
fn test_restart_node() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_restart_node");
    let slots_per_epoch = MINIMUM_SLOTS_PER_EPOCH * 2;
    let ticks_per_slot = 16;
    let validator_config = ValidatorConfig::default();
    let mut cluster = LocalCluster::new(
        &mut ClusterConfig {
            node_stakes: vec![100; 1],
            cluster_lamports: 100,
            validator_configs: vec![safe_clone_config(&validator_config)],
            ticks_per_slot,
            slots_per_epoch,
            stakers_slot_offset: slots_per_epoch,
            ..ClusterConfig::default()
        },
        SocketAddrSpace::Unspecified,
    );
    let nodes = cluster.get_node_pubkeys();
    cluster_tests::sleep_n_epochs(
        1.0,
        &cluster.genesis_config.poh_config,
        clock::DEFAULT_TICKS_PER_SLOT,
        slots_per_epoch,
    );
    cluster.exit_restart_node(&nodes[0], validator_config, SocketAddrSpace::Unspecified);
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
        validator_configs: make_identical_validator_configs(&ValidatorConfig::default(), 1),
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
    let cluster_nodes = discover_cluster(
        &cluster.entry_point_info.gossip,
        4,
        SocketAddrSpace::Unspecified,
    )
    .unwrap();
    assert_eq!(cluster_nodes.len(), 4);
}

#[test]
#[serial]
fn test_mainnet_beta_cluster_type() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);

    let mut config = ClusterConfig {
        cluster_type: ClusterType::MainnetBeta,
        node_stakes: vec![100; 1],
        cluster_lamports: 1_000,
        validator_configs: make_identical_validator_configs(&ValidatorConfig::default(), 1),
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
    let cluster_nodes = discover_cluster(
        &cluster.entry_point_info.gossip,
        1,
        SocketAddrSpace::Unspecified,
    )
    .unwrap();
    assert_eq!(cluster_nodes.len(), 1);

    let client = create_client(
        cluster.entry_point_info.client_facing_addr(),
        VALIDATOR_PORT_RANGE,
    );

    // Programs that are available at epoch 0
    for program_id in [
        &solana_config_program::id(),
        &solana_sdk::system_program::id(),
        &solana_sdk::stake::program::id(),
        &solana_vote_program::id(),
        &solana_sdk::bpf_loader_deprecated::id(),
        &solana_sdk::bpf_loader::id(),
        &solana_sdk::bpf_loader_upgradeable::id(),
    ]
    .iter()
    {
        assert_matches!(
            (
                program_id,
                client
                    .get_account_with_commitment(program_id, CommitmentConfig::processed())
                    .unwrap()
            ),
            (_program_id, Some(_))
        );
    }

    // Programs that are not available at epoch 0
    for program_id in [].iter() {
        assert_eq!(
            (
                program_id,
                client
                    .get_account_with_commitment(program_id, CommitmentConfig::processed())
                    .unwrap()
            ),
            (program_id, None)
        );
    }
}

#[test]
#[serial]
fn test_consistency_halt() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
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

    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    sleep(Duration::from_millis(5000));
    let cluster_nodes = discover_cluster(
        &cluster.entry_point_info.gossip,
        1,
        SocketAddrSpace::Unspecified,
    )
    .unwrap();
    info!("num_nodes: {}", cluster_nodes.len());

    // Add a validator with the leader as trusted, it should halt when it detects
    // mismatch.
    let mut validator_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);

    let mut known_validators = HashSet::new();
    known_validators.insert(cluster_nodes[0].id);

    validator_snapshot_test_config
        .validator_config
        .known_validators = Some(known_validators);
    validator_snapshot_test_config
        .validator_config
        .halt_on_known_validators_accounts_hash_mismatch = true;

    warn!("adding a validator");
    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        validator_stake as u64,
        Arc::new(Keypair::new()),
        None,
        SocketAddrSpace::Unspecified,
    );
    let num_nodes = 2;
    assert_eq!(
        discover_cluster(
            &cluster.entry_point_info.gossip,
            num_nodes,
            SocketAddrSpace::Unspecified
        )
        .unwrap()
        .len(),
        num_nodes
    );

    // Check for only 1 node on the network.
    let mut encountered_error = false;
    loop {
        let discover = discover_cluster(
            &cluster.entry_point_info.gossip,
            2,
            SocketAddrSpace::Unspecified,
        );
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
    solana_logger::setup_with_default(RUST_LOG_FILTER);
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
        validator_configs: make_identical_validator_configs(
            &leader_snapshot_test_config.validator_config,
            1,
        ),
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let snapshot_archives_dir = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .as_ref()
        .unwrap()
        .snapshot_archives_dir;

    trace!("Waiting for snapshot");
    let full_snapshot_archive_info = cluster.wait_for_next_full_snapshot(snapshot_archives_dir);
    trace!("found: {}", full_snapshot_archive_info.path().display());

    // Download the snapshot, then boot a validator from it.
    download_snapshot_archive(
        &cluster.entry_point_info.rpc,
        snapshot_archives_dir,
        (
            full_snapshot_archive_info.slot(),
            *full_snapshot_archive_info.hash(),
        ),
        SnapshotType::FullSnapshot,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .as_ref()
            .unwrap()
            .maximum_full_snapshot_archives_to_retain,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .as_ref()
            .unwrap()
            .maximum_incremental_snapshot_archives_to_retain,
        false,
        &mut None,
    )
    .unwrap();

    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        stake,
        Arc::new(Keypair::new()),
        None,
        SocketAddrSpace::Unspecified,
    );
}

#[test]
#[serial]
fn test_incremental_snapshot_download() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    // First set up the cluster with 1 node
    let accounts_hash_interval = 3;
    let incremental_snapshot_interval = accounts_hash_interval * 3;
    let full_snapshot_interval = incremental_snapshot_interval * 3;
    let num_account_paths = 3;

    let leader_snapshot_test_config = SnapshotValidatorConfig::new(
        full_snapshot_interval,
        incremental_snapshot_interval,
        accounts_hash_interval,
        num_account_paths,
    );
    let validator_snapshot_test_config = SnapshotValidatorConfig::new(
        full_snapshot_interval,
        incremental_snapshot_interval,
        accounts_hash_interval,
        num_account_paths,
    );

    let stake = 10_000;
    let mut config = ClusterConfig {
        node_stakes: vec![stake],
        cluster_lamports: 1_000_000,
        validator_configs: make_identical_validator_configs(
            &leader_snapshot_test_config.validator_config,
            1,
        ),
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let snapshot_archives_dir = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .as_ref()
        .unwrap()
        .snapshot_archives_dir;

    debug!("snapshot config:\n\tfull snapshot interval: {}\n\tincremental snapshot interval: {}\n\taccounts hash interval: {}",
           full_snapshot_interval,
           incremental_snapshot_interval,
           accounts_hash_interval);
    debug!(
        "leader config:\n\tbank snapshots dir: {}\n\tsnapshot archives dir: {}",
        leader_snapshot_test_config
            .bank_snapshots_dir
            .path()
            .display(),
        leader_snapshot_test_config
            .snapshot_archives_dir
            .path()
            .display(),
    );
    debug!(
        "validator config:\n\tbank snapshots dir: {}\n\tsnapshot archives dir: {}",
        validator_snapshot_test_config
            .bank_snapshots_dir
            .path()
            .display(),
        validator_snapshot_test_config
            .snapshot_archives_dir
            .path()
            .display(),
    );

    trace!("Waiting for snapshots");
    let (incremental_snapshot_archive_info, full_snapshot_archive_info) =
        cluster.wait_for_next_incremental_snapshot(snapshot_archives_dir);
    trace!(
        "found: {} and {}",
        full_snapshot_archive_info.path().display(),
        incremental_snapshot_archive_info.path().display()
    );

    // Download the snapshots, then boot a validator from them.
    download_snapshot_archive(
        &cluster.entry_point_info.rpc,
        snapshot_archives_dir,
        (
            full_snapshot_archive_info.slot(),
            *full_snapshot_archive_info.hash(),
        ),
        SnapshotType::FullSnapshot,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .as_ref()
            .unwrap()
            .maximum_full_snapshot_archives_to_retain,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .as_ref()
            .unwrap()
            .maximum_incremental_snapshot_archives_to_retain,
        false,
        &mut None,
    )
    .unwrap();

    download_snapshot_archive(
        &cluster.entry_point_info.rpc,
        snapshot_archives_dir,
        (
            incremental_snapshot_archive_info.slot(),
            *incremental_snapshot_archive_info.hash(),
        ),
        SnapshotType::IncrementalSnapshot(incremental_snapshot_archive_info.base_slot()),
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .as_ref()
            .unwrap()
            .maximum_full_snapshot_archives_to_retain,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .as_ref()
            .unwrap()
            .maximum_incremental_snapshot_archives_to_retain,
        false,
        &mut None,
    )
    .unwrap();

    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        stake,
        Arc::new(Keypair::new()),
        None,
        SocketAddrSpace::Unspecified,
    );
}

/// Test the scenario where a node starts up from a snapshot and its blockstore has enough new
/// roots that cross the full snapshot interval.  In this scenario, the node needs to take a full
/// snapshot while processing the blockstore so that once the background services start up, there
/// is the correct full snapshot available to take subsequent incremental snapshots.
///
/// For this test...
/// - Start a leader node and run it long enough to take a full and incremental snapshot
/// - Download those snapshots to a validator node
/// - Copy the validator snapshots to a back up directory
/// - Start up the validator node
/// - Wait for the validator node to see enough root slots to cross the full snapshot interval
/// - Delete the snapshots on the validator node and restore the ones from the backup
/// - Restart the validator node to trigger the scenario we're trying to test
/// - Wait for the validator node to generate a new incremental snapshot
/// - Copy the new incremental snapshot (and its associated full snapshot) to another new validator
/// - Start up this new validator to ensure the snapshots from ^^^ are good
#[test]
#[serial]
fn test_incremental_snapshot_download_with_crossing_full_snapshot_interval_at_startup() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    // If these intervals change, also make sure to change the loop timers accordingly.
    let accounts_hash_interval = 3;
    let incremental_snapshot_interval = accounts_hash_interval * 3;
    let full_snapshot_interval = incremental_snapshot_interval * 3;

    let num_account_paths = 3;
    let leader_snapshot_test_config = SnapshotValidatorConfig::new(
        full_snapshot_interval,
        incremental_snapshot_interval,
        accounts_hash_interval,
        num_account_paths,
    );
    let validator_snapshot_test_config = SnapshotValidatorConfig::new(
        full_snapshot_interval,
        incremental_snapshot_interval,
        accounts_hash_interval,
        num_account_paths,
    );
    let stake = 10_000;
    let mut config = ClusterConfig {
        node_stakes: vec![stake],
        cluster_lamports: 1_000_000,
        validator_configs: make_identical_validator_configs(
            &leader_snapshot_test_config.validator_config,
            1,
        ),
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    debug!("snapshot config:\n\tfull snapshot interval: {}\n\tincremental snapshot interval: {}\n\taccounts hash interval: {}",
           full_snapshot_interval,
           incremental_snapshot_interval,
           accounts_hash_interval);
    debug!(
        "leader config:\n\tbank snapshots dir: {}\n\tsnapshot archives dir: {}",
        leader_snapshot_test_config
            .bank_snapshots_dir
            .path()
            .display(),
        leader_snapshot_test_config
            .snapshot_archives_dir
            .path()
            .display(),
    );
    debug!(
        "validator config:\n\tbank snapshots dir: {}\n\tsnapshot archives dir: {}",
        validator_snapshot_test_config
            .bank_snapshots_dir
            .path()
            .display(),
        validator_snapshot_test_config
            .snapshot_archives_dir
            .path()
            .display(),
    );

    info!("Waiting for leader to create snapshots...");
    let (incremental_snapshot_archive_info, full_snapshot_archive_info) =
        LocalCluster::wait_for_next_incremental_snapshot(
            &cluster,
            leader_snapshot_test_config.snapshot_archives_dir.path(),
        );
    debug!(
        "Found snapshots:\n\tfull snapshot: {}\n\tincremental snapshot: {}",
        full_snapshot_archive_info.path().display(),
        incremental_snapshot_archive_info.path().display()
    );
    assert_eq!(
        full_snapshot_archive_info.slot(),
        incremental_snapshot_archive_info.base_slot()
    );

    // Download the snapshots, then boot a validator from them.
    info!("Downloading full snapshot to validator...");
    download_snapshot_archive(
        &cluster.entry_point_info.rpc,
        validator_snapshot_test_config.snapshot_archives_dir.path(),
        (
            full_snapshot_archive_info.slot(),
            *full_snapshot_archive_info.hash(),
        ),
        SnapshotType::FullSnapshot,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .as_ref()
            .unwrap()
            .maximum_full_snapshot_archives_to_retain,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .as_ref()
            .unwrap()
            .maximum_incremental_snapshot_archives_to_retain,
        false,
        &mut None,
    )
    .unwrap();
    let downloaded_full_snapshot_archive_info =
        snapshot_utils::get_highest_full_snapshot_archive_info(
            validator_snapshot_test_config.snapshot_archives_dir.path(),
        )
        .unwrap();
    debug!(
        "Downloaded full snapshot, slot: {}",
        downloaded_full_snapshot_archive_info.slot()
    );

    info!("Downloading incremental snapshot to validator...");
    download_snapshot_archive(
        &cluster.entry_point_info.rpc,
        validator_snapshot_test_config.snapshot_archives_dir.path(),
        (
            incremental_snapshot_archive_info.slot(),
            *incremental_snapshot_archive_info.hash(),
        ),
        SnapshotType::IncrementalSnapshot(incremental_snapshot_archive_info.base_slot()),
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .as_ref()
            .unwrap()
            .maximum_full_snapshot_archives_to_retain,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .as_ref()
            .unwrap()
            .maximum_incremental_snapshot_archives_to_retain,
        false,
        &mut None,
    )
    .unwrap();
    let downloaded_incremental_snapshot_archive_info =
        snapshot_utils::get_highest_incremental_snapshot_archive_info(
            validator_snapshot_test_config.snapshot_archives_dir.path(),
            full_snapshot_archive_info.slot(),
        )
        .unwrap();
    debug!(
        "Downloaded incremental snapshot, slot: {}, base slot: {}",
        downloaded_incremental_snapshot_archive_info.slot(),
        downloaded_incremental_snapshot_archive_info.base_slot(),
    );
    assert_eq!(
        downloaded_full_snapshot_archive_info.slot(),
        downloaded_incremental_snapshot_archive_info.base_slot()
    );

    // closure to copy files in a directory to another directory
    let copy_files = |from: &Path, to: &Path| {
        trace!(
            "copying files from dir {}, to dir {}",
            from.display(),
            to.display()
        );
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                continue;
            }
            let from_file_path = entry.path();
            let to_file_path = to.join(from_file_path.file_name().unwrap());
            trace!(
                "\t\tcopying file from {} to {}...",
                from_file_path.display(),
                to_file_path.display()
            );
            fs::copy(from_file_path, to_file_path).unwrap();
        }
    };
    // closure to delete files in a directory
    let delete_files = |dir: &Path| {
        trace!("deleting files in dir {}", dir.display());
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                continue;
            }
            let file_path = entry.path();
            trace!("\t\tdeleting file {}...", file_path.display());
            fs::remove_file(file_path).unwrap();
        }
    };

    // After downloading the snapshots, copy them over to a backup directory.  Later we'll need to
    // restart the node and guarantee that the only snapshots present are these initial ones.  So,
    // the easiest way to do that is create a backup now, delete the ones on the node before
    // restart, then copy the backup ones over again.
    let backup_validator_snapshot_archives_dir = tempfile::tempdir_in(farf_dir()).unwrap();
    trace!(
        "Backing up validator snapshots to dir: {}...",
        backup_validator_snapshot_archives_dir.path().display()
    );
    copy_files(
        validator_snapshot_test_config.snapshot_archives_dir.path(),
        backup_validator_snapshot_archives_dir.path(),
    );

    info!("Starting a new validator...");
    let validator_identity = Arc::new(Keypair::new());
    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        stake,
        validator_identity.clone(),
        None,
        SocketAddrSpace::Unspecified,
    );

    // To ensure that a snapshot will be taken during startup, the blockstore needs to have roots
    // that cross a full snapshot interval.
    info!("Waiting for the validator to see enough slots to cross a full snapshot interval...");
    let starting_slot = incremental_snapshot_archive_info.slot();
    let timer = Instant::now();
    loop {
        let validator_current_slot = cluster
            .get_validator_client(&validator_identity.pubkey())
            .unwrap()
            .get_slot_with_commitment(CommitmentConfig::finalized())
            .unwrap();
        if validator_current_slot > (starting_slot + full_snapshot_interval) {
            break;
        }
        assert!(
            timer.elapsed() < Duration::from_secs(30),
            "It should not take longer than 30 seconds to cross the next full snapshot interval."
        );
        std::thread::yield_now();
    }
    trace!("Waited {:?}", timer.elapsed());

    // Get the highest full snapshot archive info for the validator, now that it has crossed the
    // next full snapshot interval.  We are going to use this to look up the same snapshot on the
    // leader, which we'll then use to compare to the full snapshot the validator will create
    // during startup.  This ensures the snapshot creation process during startup is correct.
    //
    // Putting this all in its own block so its clear we're only intended to keep the leader's info
    let leader_full_snapshot_archive_info_for_comparison = {
        let validator_full_snapshot = snapshot_utils::get_highest_full_snapshot_archive_info(
            validator_snapshot_test_config.snapshot_archives_dir.path(),
        )
        .unwrap();

        // Now get the same full snapshot on the LEADER that we just got from the validator
        let mut leader_full_snapshots = snapshot_utils::get_full_snapshot_archives(
            leader_snapshot_test_config.snapshot_archives_dir.path(),
        );
        leader_full_snapshots.retain(|full_snapshot| {
            full_snapshot.slot() == validator_full_snapshot.slot()
                && full_snapshot.hash() == validator_full_snapshot.hash()
        });

        // NOTE: If this unwrap() ever fails, it may be that the leader's old full snapshot archives
        // were purged.  If that happens, increase the maximum_full_snapshot_archives_to_retain
        // in the leader's Snapshotconfig.
        let leader_full_snapshot = leader_full_snapshots.first().unwrap();

        // And for sanity, the full snapshot from the leader and the validator MUST be the same
        assert_eq!(
            (
                validator_full_snapshot.slot(),
                validator_full_snapshot.hash()
            ),
            (leader_full_snapshot.slot(), leader_full_snapshot.hash())
        );

        leader_full_snapshot.clone()
    };

    trace!(
        "Delete all the snapshots on the validator and restore the originals from the backup..."
    );
    delete_files(validator_snapshot_test_config.snapshot_archives_dir.path());
    copy_files(
        backup_validator_snapshot_archives_dir.path(),
        validator_snapshot_test_config.snapshot_archives_dir.path(),
    );

    // Get the highest full snapshot slot *before* restarting, as a comparison
    let validator_full_snapshot_slot_at_startup =
        snapshot_utils::get_highest_full_snapshot_archive_slot(
            validator_snapshot_test_config.snapshot_archives_dir.path(),
        )
        .unwrap();

    info!("Restarting the validator...");
    let validator_info = cluster.exit_node(&validator_identity.pubkey());
    cluster.restart_node(
        &validator_identity.pubkey(),
        validator_info,
        SocketAddrSpace::Unspecified,
    );

    // Now, we want to ensure that the validator can make a new incremental snapshot based on the
    // new full snapshot that was created during the restart.
    let timer = Instant::now();
    let (
        validator_highest_full_snapshot_archive_info,
        _validator_highest_incremental_snapshot_archive_info,
    ) = loop {
        if let Some(highest_full_snapshot_info) =
            snapshot_utils::get_highest_full_snapshot_archive_info(
                validator_snapshot_test_config.snapshot_archives_dir.path(),
            )
        {
            if highest_full_snapshot_info.slot() > validator_full_snapshot_slot_at_startup {
                if let Some(highest_incremental_snapshot_info) =
                    snapshot_utils::get_highest_incremental_snapshot_archive_info(
                        validator_snapshot_test_config.snapshot_archives_dir.path(),
                        highest_full_snapshot_info.slot(),
                    )
                {
                    info!("Success! Made new full and incremental snapshots!");
                    trace!(
                        "Full snapshot slot: {}, incremental snapshot slot: {}",
                        highest_full_snapshot_info.slot(),
                        highest_incremental_snapshot_info.slot(),
                    );
                    break (
                        highest_full_snapshot_info,
                        highest_incremental_snapshot_info,
                    );
                }
            }
        }
        assert!(
            timer.elapsed() < Duration::from_secs(10),
            "It should not take longer than 10 seconds to cross the next incremental snapshot interval."
        );
        std::thread::yield_now();
    };
    trace!("Waited {:?}", timer.elapsed());

    // Check to make sure that the full snapshot the validator created during startup is the same
    // as the snapshot the leader created.
    // NOTE: If the assert fires and the _slots_ don't match (specifically are off by a full
    // snapshot interval), then that means the loop to get the
    // `validator_highest_full_snapshot_archive_info` saw the wrong one, and that may've been due
    // to some weird scheduling/delays on the machine running the test.  Run the test again.  If
    // this ever fails repeatedly then the test will need to be modified to handle this case.
    assert_eq!(
        (
            validator_highest_full_snapshot_archive_info.slot(),
            validator_highest_full_snapshot_archive_info.hash()
        ),
        (
            leader_full_snapshot_archive_info_for_comparison.slot(),
            leader_full_snapshot_archive_info_for_comparison.hash()
        )
    );

    // And lastly, startup another node with the new snapshots to ensure they work
    let final_validator_snapshot_test_config = SnapshotValidatorConfig::new(
        full_snapshot_interval,
        incremental_snapshot_interval,
        accounts_hash_interval,
        num_account_paths,
    );

    // Copy over the snapshots to the new node, but need to remove the tmp snapshot dir so it
    // doesn't break the simple copy_files closure.
    snapshot_utils::remove_tmp_snapshot_archives(
        validator_snapshot_test_config.snapshot_archives_dir.path(),
    );
    copy_files(
        validator_snapshot_test_config.snapshot_archives_dir.path(),
        final_validator_snapshot_test_config
            .snapshot_archives_dir
            .path(),
    );

    info!("Starting final validator...");
    let final_validator_identity = Arc::new(Keypair::new());
    cluster.add_validator(
        &final_validator_snapshot_test_config.validator_config,
        stake,
        final_validator_identity,
        None,
        SocketAddrSpace::Unspecified,
    );

    // Success!
}

#[allow(unused_attributes)]
#[test]
#[serial]
fn test_snapshot_restart_tower() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
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
            safe_clone_config(&leader_snapshot_test_config.validator_config),
            safe_clone_config(&validator_snapshot_test_config.validator_config),
        ],
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    // Let the nodes run for a while, then stop one of the validators
    sleep(Duration::from_millis(5000));
    let all_pubkeys = cluster.get_node_pubkeys();
    let validator_id = all_pubkeys
        .into_iter()
        .find(|x| *x != cluster.entry_point_info.id)
        .unwrap();
    let validator_info = cluster.exit_node(&validator_id);

    // Get slot after which this was generated
    let snapshot_archives_dir = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .as_ref()
        .unwrap()
        .snapshot_archives_dir;

    let full_snapshot_archive_info = cluster.wait_for_next_full_snapshot(snapshot_archives_dir);

    // Copy archive to validator's snapshot output directory
    let validator_archive_path = snapshot_utils::build_full_snapshot_archive_path(
        validator_snapshot_test_config
            .snapshot_archives_dir
            .path()
            .to_path_buf(),
        full_snapshot_archive_info.slot(),
        full_snapshot_archive_info.hash(),
        full_snapshot_archive_info.archive_format(),
    );
    fs::hard_link(full_snapshot_archive_info.path(), &validator_archive_path).unwrap();

    // Restart validator from snapshot, the validator's tower state in this snapshot
    // will contain slots < the root bank of the snapshot. Validator should not panic.
    cluster.restart_node(&validator_id, validator_info, SocketAddrSpace::Unspecified);

    // Test cluster can still make progress and get confirmations in tower
    // Use the restarted node as the discovery point so that we get updated
    // validator's ContactInfo
    let restarted_node_info = cluster.get_contact_info(&validator_id).unwrap();
    cluster_tests::spend_and_verify_all_nodes(
        restarted_node_info,
        &cluster.funding_keypair,
        1,
        HashSet::new(),
        SocketAddrSpace::Unspecified,
    );
}

#[test]
#[serial]
fn test_snapshots_blockstore_floor() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    // First set up the cluster with 1 snapshotting leader
    let snapshot_interval_slots = 10;
    let num_account_paths = 4;

    let leader_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
    let mut validator_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);

    let snapshot_archives_dir = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .as_ref()
        .unwrap()
        .snapshot_archives_dir;

    let mut config = ClusterConfig {
        node_stakes: vec![10000],
        cluster_lamports: 100_000,
        validator_configs: make_identical_validator_configs(
            &leader_snapshot_test_config.validator_config,
            1,
        ),
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    trace!("Waiting for snapshot tar to be generated with slot",);

    let archive_info = loop {
        let archive =
            snapshot_utils::get_highest_full_snapshot_archive_info(&snapshot_archives_dir);
        if archive.is_some() {
            trace!("snapshot exists");
            break archive.unwrap();
        }
        sleep(Duration::from_millis(5000));
    };

    // Copy archive to validator's snapshot output directory
    let validator_archive_path = snapshot_utils::build_full_snapshot_archive_path(
        validator_snapshot_test_config
            .snapshot_archives_dir
            .path()
            .to_path_buf(),
        archive_info.slot(),
        archive_info.hash(),
        ArchiveFormat::TarBzip2,
    );
    fs::hard_link(archive_info.path(), &validator_archive_path).unwrap();
    let slot_floor = archive_info.slot();

    // Start up a new node from a snapshot
    let validator_stake = 5;

    let cluster_nodes = discover_cluster(
        &cluster.entry_point_info.gossip,
        1,
        SocketAddrSpace::Unspecified,
    )
    .unwrap();
    let mut known_validators = HashSet::new();
    known_validators.insert(cluster_nodes[0].id);
    validator_snapshot_test_config
        .validator_config
        .known_validators = Some(known_validators);

    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        validator_stake,
        Arc::new(Keypair::new()),
        None,
        SocketAddrSpace::Unspecified,
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
        if let Ok(slot) = validator_client.get_slot_with_commitment(CommitmentConfig::processed()) {
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
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let snapshot_interval_slots = 10;
    let num_account_paths = 1;
    let mut snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
    let snapshot_archives_dir = &snapshot_test_config
        .validator_config
        .snapshot_config
        .as_ref()
        .unwrap()
        .snapshot_archives_dir;

    // Set up the cluster with 1 snapshotting validator
    let mut all_account_storage_dirs = vec![vec![]];

    std::mem::swap(
        &mut all_account_storage_dirs[0],
        &mut snapshot_test_config.account_storage_dirs,
    );

    let mut config = ClusterConfig {
        node_stakes: vec![10000],
        cluster_lamports: 100_000,
        validator_configs: make_identical_validator_configs(
            &snapshot_test_config.validator_config,
            1,
        ),
        ..ClusterConfig::default()
    };

    // Create and reboot the node from snapshot `num_runs` times
    let num_runs = 3;
    let mut expected_balances = HashMap::new();
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
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

        cluster.wait_for_next_full_snapshot(snapshot_archives_dir);

        // Create new account paths since validator exit is not guaranteed to cleanup RPC threads,
        // which may delete the old accounts on exit at any point
        let (new_account_storage_dirs, new_account_storage_paths) =
            generate_account_paths(num_account_paths);
        all_account_storage_dirs.push(new_account_storage_dirs);
        snapshot_test_config.validator_config.account_paths = new_account_storage_paths;

        // Restart node
        trace!("Restarting cluster from snapshot");
        let nodes = cluster.get_node_pubkeys();
        cluster.exit_restart_node(
            &nodes[0],
            safe_clone_config(&snapshot_test_config.validator_config),
            SocketAddrSpace::Unspecified,
        );

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
            SocketAddrSpace::Unspecified,
        );
    }
}

#[test]
#[serial]
#[allow(unused_attributes)]
#[ignore]
fn test_fail_entry_verification_leader() {
    let leader_stake = (DUPLICATE_THRESHOLD * 100.0) as u64 + 1;
    let validator_stake1 = (100 - leader_stake) / 2;
    let validator_stake2 = 100 - leader_stake - validator_stake1;
    let (cluster, _) = test_faulty_node(
        BroadcastStageType::FailEntryVerification,
        vec![leader_stake, validator_stake1, validator_stake2],
    );
    cluster.check_for_new_roots(
        16,
        "test_fail_entry_verification_leader",
        SocketAddrSpace::Unspecified,
    );
}

#[test]
#[serial]
#[ignore]
#[allow(unused_attributes)]
fn test_fake_shreds_broadcast_leader() {
    let node_stakes = vec![300, 100];
    let (cluster, _) = test_faulty_node(BroadcastStageType::BroadcastFakeShreds, node_stakes);
    cluster.check_for_new_roots(
        16,
        "test_fake_shreds_broadcast_leader",
        SocketAddrSpace::Unspecified,
    );
}

#[test]
#[serial]
#[allow(unused_attributes)]
fn test_duplicate_shreds_broadcast_leader() {
    // Create 4 nodes:
    // 1) Bad leader sending different versions of shreds to both of the other nodes
    // 2) 1 node who's voting behavior in gossip
    // 3) 1 validator gets the same version as the leader, will see duplicate confirmation
    // 4) 1 validator will not get the same version as the leader. For each of these
    // duplicate slots `S` either:
    //    a) The leader's version of `S` gets > DUPLICATE_THRESHOLD of votes in gossip and so this
    //       node will repair that correct version
    //    b) A descendant `D` of some version of `S` gets > DUPLICATE_THRESHOLD votes in gossip,
    //       but no version of `S` does. Then the node will not know to repair the right version
    //       by just looking at gossip, but will instead have to use EpochSlots repair after
    //       detecting that a descendant does not chain to its version of `S`, and marks that descendant
    //       dead.
    //   Scenarios a) or b) are triggered by our node in 2) who's voting behavior we control.

    // Critical that bad_leader_stake + good_node_stake < DUPLICATE_THRESHOLD and that
    // bad_leader_stake + good_node_stake + our_node_stake > DUPLICATE_THRESHOLD so that
    // our vote is the determining factor
    let bad_leader_stake = 10000000000;
    // Ensure that the good_node_stake is always on the critical path, and the partition node
    // should never be on the critical path. This way, none of the bad shreds sent to the partition
    // node corrupt the good node.
    let good_node_stake = 500000;
    let our_node_stake = 10000000000;
    let partition_node_stake = 1;

    let node_stakes = vec![
        bad_leader_stake,
        partition_node_stake,
        good_node_stake,
        // Needs to be last in the vector, so that we can
        // find the id of this node. See call to `test_faulty_node`
        // below for more details.
        our_node_stake,
    ];
    assert_eq!(*node_stakes.last().unwrap(), our_node_stake);
    let total_stake: u64 = node_stakes.iter().sum();

    assert!(
        ((bad_leader_stake + good_node_stake) as f64 / total_stake as f64) < DUPLICATE_THRESHOLD
    );
    assert!(
        (bad_leader_stake + good_node_stake + our_node_stake) as f64 / total_stake as f64
            > DUPLICATE_THRESHOLD
    );

    // Important that the partition node stake is the smallest so that it gets selected
    // for the partition.
    assert!(partition_node_stake < our_node_stake && partition_node_stake < good_node_stake);

    // 1) Set up the cluster
    let (mut cluster, validator_keys) = test_faulty_node(
        BroadcastStageType::BroadcastDuplicates(BroadcastDuplicatesConfig {
            stake_partition: partition_node_stake,
        }),
        node_stakes,
    );

    // This is why it's important our node was last in `node_stakes`
    let our_id = validator_keys.last().unwrap().pubkey();

    // 2) Kill our node and start up a thread to simulate votes to control our voting behavior
    let our_info = cluster.exit_node(&our_id);
    let node_keypair = our_info.info.keypair;
    let vote_keypair = our_info.info.voting_keypair;
    let bad_leader_id = cluster.entry_point_info.id;
    let bad_leader_ledger_path = cluster.validators[&bad_leader_id].info.ledger_path.clone();
    info!("our node id: {}", node_keypair.pubkey());

    // 3) Start up a spy to listen for votes
    let exit = Arc::new(AtomicBool::new(false));
    let (gossip_service, _tcp_listener, cluster_info) = gossip_service::make_gossip_node(
        // Need to use our validator's keypair to gossip EpochSlots and votes for our
        // node later.
        Keypair::from_bytes(&node_keypair.to_bytes()).unwrap(),
        Some(&cluster.entry_point_info.gossip),
        &exit,
        None,
        0,
        false,
        SocketAddrSpace::Unspecified,
    );

    let t_voter = {
        let exit = exit.clone();
        std::thread::spawn(move || {
            let mut cursor = Cursor::default();
            let mut max_vote_slot = 0;
            let mut gossip_vote_index = 0;
            loop {
                if exit.load(Ordering::Relaxed) {
                    return;
                }

                let (labels, votes) = cluster_info.get_votes_with_labels(&mut cursor);
                let mut parsed_vote_iter: Vec<_> = labels
                    .into_iter()
                    .zip(votes.into_iter())
                    .filter_map(|(label, leader_vote_tx)| {
                        // Filter out votes not from the bad leader
                        if label.pubkey() == bad_leader_id {
                            let vote = vote_transaction::parse_vote_transaction(&leader_vote_tx)
                                .map(|(_, vote, _)| vote)
                                .unwrap();
                            // Filter out empty votes
                            if !vote.is_empty() {
                                Some((vote, leader_vote_tx))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect();

                parsed_vote_iter.sort_by(|(vote, _), (vote2, _)| {
                    vote.last_voted_slot()
                        .unwrap()
                        .cmp(&vote2.last_voted_slot().unwrap())
                });

                for (parsed_vote, leader_vote_tx) in &parsed_vote_iter {
                    if let Some(latest_vote_slot) = parsed_vote.last_voted_slot() {
                        info!("received vote for {}", latest_vote_slot);
                        // Add to EpochSlots. Mark all slots frozen between slot..=max_vote_slot.
                        if latest_vote_slot > max_vote_slot {
                            let new_epoch_slots: Vec<Slot> =
                                (max_vote_slot + 1..latest_vote_slot + 1).collect();
                            info!(
                                "Simulating epoch slots from our node: {:?}",
                                new_epoch_slots
                            );
                            cluster_info.push_epoch_slots(&new_epoch_slots);
                            max_vote_slot = latest_vote_slot;
                        }

                        // Only vote on even slots. Note this may violate lockouts if the
                        // validator started voting on a different fork before we could exit
                        // it above.
                        let vote_hash = parsed_vote.hash();
                        if latest_vote_slot % 2 == 0 {
                            info!(
                                "Simulating vote from our node on slot {}, hash {}",
                                latest_vote_slot, vote_hash
                            );

                            // Add all recent vote slots on this fork to allow cluster to pass
                            // vote threshold checks in replay. Note this will instantly force a
                            // root by this validator, but we're not concerned with lockout violations
                            // by this validator so it's fine.
                            let leader_blockstore = open_blockstore(&bad_leader_ledger_path);
                            let mut vote_slots: Vec<Slot> = AncestorIterator::new_inclusive(
                                latest_vote_slot,
                                &leader_blockstore,
                            )
                            .take(MAX_LOCKOUT_HISTORY)
                            .collect();
                            vote_slots.reverse();
                            let vote_tx = vote_transaction::new_vote_transaction(
                                vote_slots,
                                vote_hash,
                                leader_vote_tx.message.recent_blockhash,
                                &node_keypair,
                                &vote_keypair,
                                &vote_keypair,
                                None,
                            );
                            gossip_vote_index += 1;
                            gossip_vote_index %= MAX_LOCKOUT_HISTORY;
                            cluster_info.push_vote_at_index(vote_tx, gossip_vote_index as u8)
                        }
                    }
                    // Give vote some time to propagate
                    sleep(Duration::from_millis(100));
                }

                if parsed_vote_iter.is_empty() {
                    sleep(Duration::from_millis(100));
                }
            }
        })
    };

    // 4) Check that the cluster is making progress
    cluster.check_for_new_roots(
        16,
        "test_duplicate_shreds_broadcast_leader",
        SocketAddrSpace::Unspecified,
    );

    // Clean up threads
    exit.store(true, Ordering::Relaxed);
    t_voter.join().unwrap();
    gossip_service.join().unwrap();
}

fn test_faulty_node(
    faulty_node_type: BroadcastStageType,
    node_stakes: Vec<u64>,
) -> (LocalCluster, Vec<Arc<Keypair>>) {
    solana_logger::setup_with_default("solana_local_cluster=info");
    let num_nodes = node_stakes.len();

    let error_validator_config = ValidatorConfig {
        broadcast_stage_type: faulty_node_type,
        ..ValidatorConfig::default()
    };
    let mut validator_configs = Vec::with_capacity(num_nodes);

    // First validator is the bootstrap leader with the malicious broadcast logic.
    validator_configs.push(error_validator_config);
    validator_configs.resize_with(num_nodes, ValidatorConfig::default);

    let mut validator_keys = Vec::with_capacity(num_nodes);
    validator_keys.resize_with(num_nodes, || (Arc::new(Keypair::new()), true));

    assert_eq!(node_stakes.len(), num_nodes);
    assert_eq!(validator_keys.len(), num_nodes);

    let mut cluster_config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes,
        validator_configs,
        validator_keys: Some(validator_keys.clone()),
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };

    let cluster = LocalCluster::new(&mut cluster_config, SocketAddrSpace::Unspecified);
    let validator_keys: Vec<Arc<Keypair>> = validator_keys
        .into_iter()
        .map(|(keypair, _)| keypair)
        .collect();

    (cluster, validator_keys)
}

#[test]
fn test_wait_for_max_stake() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let validator_config = ValidatorConfig::default();
    let mut config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100; 4],
        validator_configs: make_identical_validator_configs(&validator_config, 4),
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
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
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let validator_config = ValidatorConfig {
        voting_disabled: true,
        ..ValidatorConfig::default()
    };
    let mut config = ClusterConfig {
        cluster_lamports: 10_000,
        node_stakes: vec![100],
        validator_configs: vec![validator_config],
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
    let client = cluster
        .get_validator_client(&cluster.entry_point_info.id)
        .unwrap();
    loop {
        let last_slot = client
            .get_slot_with_commitment(CommitmentConfig::processed())
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
        assert_eq!(parent, Some(expected_parent as u64));
    }
}

#[test]
#[serial]
fn test_optimistic_confirmation_violation_detection() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
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
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default(),
            node_stakes.len(),
        ),
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
    let entry_point_id = cluster.entry_point_info.id;
    // Let the nodes run for a while. Wait for validators to vote on slot `S`
    // so that the vote on `S-1` is definitely in gossip and optimistic confirmation is
    // detected on slot `S-1` for sure, then stop the heavier of the two
    // validators
    let client = cluster.get_validator_client(&entry_point_id).unwrap();
    let mut prev_voted_slot = 0;
    loop {
        let last_voted_slot = client
            .get_slot_with_commitment(CommitmentConfig::processed())
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
        // Necessary otherwise tower will inform this validator that it's latest
        // vote is on slot `prev_voted_slot`. This will then prevent this validator
        // from resetting to the parent of `prev_voted_slot` to create an alternative fork because
        // 1) Validator can't vote on earlier ancestor of last vote due to switch threshold (can't vote
        // on ancestors of last vote)
        // 2) Won't reset to this earlier ancestor becasue reset can only happen on same voted fork if
        // it's for the last vote slot or later
        remove_tower(&exited_validator_info.info.ledger_path, &entry_point_id);
        blockstore.set_dead_slot(prev_voted_slot).unwrap();
    }

    {
        // Buffer stderr to detect optimistic slot violation log
        let buf = std::env::var("OPTIMISTIC_CONF_TEST_DUMP_LOG")
            .err()
            .map(|_| BufferRedirect::stderr().unwrap());
        cluster.restart_node(
            &entry_point_id,
            exited_validator_info,
            SocketAddrSpace::Unspecified,
        );

        // Wait for a root > prev_voted_slot to be set. Because the root is on a
        // different fork than `prev_voted_slot`, then optimistic confirmation is
        // violated
        let client = cluster.get_validator_client(&entry_point_id).unwrap();
        loop {
            let last_root = client
                .get_slot_with_commitment(CommitmentConfig::finalized())
                .unwrap();
            if last_root > prev_voted_slot {
                break;
            }
            sleep(Duration::from_millis(100));
        }

        // Check to see that validator detected optimistic confirmation for
        // `prev_voted_slot` failed
        let expected_log =
            OptimisticConfirmationVerifier::format_optimistic_confirmed_slot_violation_log(
                prev_voted_slot,
            );
        // Violation detection thread can be behind so poll logs up to 10 seconds
        if let Some(mut buf) = buf {
            let start = Instant::now();
            let mut success = false;
            let mut output = String::new();
            while start.elapsed().as_secs() < 10 {
                buf.read_to_string(&mut output).unwrap();
                if output.contains(&expected_log) {
                    success = true;
                    break;
                }
                sleep(Duration::from_millis(10));
            }
            print!("{}", output);
            assert!(success);
        } else {
            panic!("dumped log and disabled testing");
        }
    }

    // Make sure validator still makes progress
    cluster_tests::check_for_new_roots(
        16,
        &[cluster.get_contact_info(&entry_point_id).unwrap().clone()],
        "test_optimistic_confirmation_violation",
    );
}

#[test]
#[serial]
fn test_validator_saves_tower() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);

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
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let validator_client = cluster.get_validator_client(&validator_id).unwrap();

    let ledger_path = cluster
        .validators
        .get(&validator_id)
        .unwrap()
        .info
        .ledger_path
        .clone();

    let file_tower_storage = FileTowerStorage::new(ledger_path.clone());

    // Wait for some votes to be generated
    let mut last_replayed_root;
    loop {
        if let Ok(slot) = validator_client.get_slot_with_commitment(CommitmentConfig::processed()) {
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
    let tower1 = Tower::restore(&file_tower_storage, &validator_id).unwrap();
    trace!("tower1: {:?}", tower1);
    assert_eq!(tower1.root(), 0);

    // Restart the validator and wait for a new root
    cluster.restart_node(&validator_id, validator_info, SocketAddrSpace::Unspecified);
    let validator_client = cluster.get_validator_client(&validator_id).unwrap();

    // Wait for the first root
    loop {
        #[allow(deprecated)]
        // This test depends on knowing the immediate root, without any delay from the commitment
        // service, so the deprecated CommitmentConfig::root() is retained
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
        .get_slot_with_commitment(CommitmentConfig::processed())
        .unwrap();
    let validator_info = cluster.exit_node(&validator_id);
    let tower2 = Tower::restore(&file_tower_storage, &validator_id).unwrap();
    trace!("tower2: {:?}", tower2);
    assert_eq!(tower2.root(), last_replayed_root);
    last_replayed_root = recent_slot;

    // Rollback saved tower to `tower1` to simulate a validator starting from a newer snapshot
    // without having to wait for that snapshot to be generated in this test
    tower1
        .save(&file_tower_storage, &validator_identity_keypair)
        .unwrap();

    cluster.restart_node(&validator_id, validator_info, SocketAddrSpace::Unspecified);
    let validator_client = cluster.get_validator_client(&validator_id).unwrap();

    // Wait for a new root, demonstrating the validator was able to make progress from the older `tower1`
    loop {
        #[allow(deprecated)]
        // This test depends on knowing the immediate root, without any delay from the commitment
        // service, so the deprecated CommitmentConfig::root() is retained
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
    let tower3 = Tower::restore(&file_tower_storage, &validator_id).unwrap();
    trace!("tower3: {:?}", tower3);
    assert!(tower3.root() > last_replayed_root);

    // Remove the tower file entirely and allow the validator to start without a tower.  It will
    // rebuild tower from its vote account contents
    remove_tower(&ledger_path, &validator_id);
    validator_info.config.require_tower = false;

    cluster.restart_node(&validator_id, validator_info, SocketAddrSpace::Unspecified);
    let validator_client = cluster.get_validator_client(&validator_id).unwrap();

    // Wait for a couple more slots to pass so another vote occurs
    let current_slot = validator_client
        .get_slot_with_commitment(CommitmentConfig::processed())
        .unwrap();
    loop {
        if let Ok(slot) = validator_client.get_slot_with_commitment(CommitmentConfig::processed()) {
            trace!("current_slot: {}, slot: {}", current_slot, slot);
            if slot > current_slot + 1 {
                break;
            }
        }
        sleep(Duration::from_millis(50));
    }

    cluster.close_preserve_ledgers();

    let tower4 = Tower::restore(&file_tower_storage, &validator_id).unwrap();
    trace!("tower4: {:?}", tower4);
    // should tower4 advance 1 slot compared to tower3????
    assert_eq!(tower4.root(), tower3.root() + 1);
}

fn open_blockstore(ledger_path: &Path) -> Blockstore {
    Blockstore::open_with_access_type(ledger_path, AccessType::TryPrimaryThenSecondary, None, true)
        .unwrap_or_else(|e| {
            panic!("Failed to open ledger at {:?}, err: {}", ledger_path, e);
        })
}

fn purge_slots(blockstore: &Blockstore, start_slot: Slot, slot_count: Slot) {
    blockstore.purge_from_next_slots(start_slot, start_slot + slot_count);
    blockstore.purge_slots(start_slot, start_slot + slot_count, PurgeType::Exact);
}

fn copy_blocks(end_slot: Slot, source: &Blockstore, dest: &Blockstore) {
    for slot in std::iter::once(end_slot).chain(AncestorIterator::new(end_slot, source)) {
        let source_meta = source.meta(slot).unwrap().unwrap();
        assert!(source_meta.is_full());

        let shreds = source.get_data_shreds_for_slot(slot, 0).unwrap();
        dest.insert_shreds(shreds, None, false).unwrap();

        let dest_meta = dest.meta(slot).unwrap().unwrap();
        assert!(dest_meta.is_full());
        assert_eq!(dest_meta.last_index, source_meta.last_index);
    }
}

fn save_tower(tower_path: &Path, tower: &Tower, node_keypair: &Keypair) {
    let file_tower_storage = FileTowerStorage::new(tower_path.to_path_buf());
    let saved_tower = SavedTower::new(tower, node_keypair).unwrap();
    file_tower_storage.store(&saved_tower).unwrap();
}

fn restore_tower(tower_path: &Path, node_pubkey: &Pubkey) -> Option<Tower> {
    let file_tower_storage = FileTowerStorage::new(tower_path.to_path_buf());

    let tower = Tower::restore(&file_tower_storage, node_pubkey);
    if let Err(tower_err) = tower {
        if tower_err.is_file_missing() {
            return None;
        } else {
            panic!("tower restore failed...: {:?}", tower_err);
        }
    }
    // actually saved tower must have at least one vote.
    Tower::restore(&file_tower_storage, node_pubkey).ok()
}

fn last_vote_in_tower(tower_path: &Path, node_pubkey: &Pubkey) -> Option<(Slot, Hash)> {
    restore_tower(tower_path, node_pubkey).map(|tower| tower.last_voted_slot_hash().unwrap())
}

// Fetches the last vote in the tower, blocking until it has also appeared in blockstore.
// Fails if tower is empty
fn wait_for_last_vote_in_tower_to_land_in_ledger(ledger_path: &Path, node_pubkey: &Pubkey) -> Slot {
    let (last_vote, _) = last_vote_in_tower(ledger_path, node_pubkey).unwrap();
    loop {
        // We reopen in a loop to make sure we get updates
        let blockstore = open_blockstore(ledger_path);
        if blockstore.is_full(last_vote) {
            break;
        }
        sleep(Duration::from_millis(100));
    }
    last_vote
}

fn root_in_tower(tower_path: &Path, node_pubkey: &Pubkey) -> Option<Slot> {
    restore_tower(tower_path, node_pubkey).map(|tower| tower.root())
}

fn remove_tower(tower_path: &Path, node_pubkey: &Pubkey) {
    let file_tower_storage = FileTowerStorage::new(tower_path.to_path_buf());
    fs::remove_file(file_tower_storage.filename(node_pubkey)).unwrap();
}

// A bit convoluted test case; but this roughly follows this test theoretical scenario:
//
// Step 1: You have validator A + B with 31% and 36% of the stake. Run only validator B:
//
//  S0 -> S1 -> S2 -> S3 (B vote)
//
// Step 2: Turn off B, and truncate the ledger after slot `S3` (simulate votes not
// landing in next slot).
// Copy ledger fully to validator A and validator C
//
// Step 3: Turn on A, and have it vote up to S3. Truncate anything past slot `S3`.
//
//  S0 -> S1 -> S2 -> S3 (A & B vote, optimistically confirmed)
//
// Step 4:
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
// Step 5:
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
    solana_logger::setup_with_default(RUST_LOG_FILTER);

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

    // Disable voting on all validators other than validator B to ensure neither of the below two
    // scenarios occur:
    // 1. If the cluster immediately forks on restart while we're killing validators A and C,
    // with Validator B on one side, and `A` and `C` on a heavier fork, it's possible that the lockouts
    // on `A` and `C`'s latest votes do not extend past validator B's latest vote. Then validator B
    // will be stuck unable to vote, but also unable generate a switching proof to the heavier fork.
    //
    // 2. Validator A doesn't vote past `next_slot_on_a` before we can kill it. This is essential
    // because if validator A votes past `next_slot_on_a`, and then we copy over validator B's ledger
    // below only for slots <= `next_slot_on_a`, validator A will not know how it's last vote chains
    // to the otehr forks, and may violate switching proofs on restart.
    let mut validator_configs =
        make_identical_validator_configs(&ValidatorConfig::default(), node_stakes.len());

    validator_configs[0].voting_disabled = true;
    validator_configs[2].voting_disabled = true;

    let mut config = ClusterConfig {
        cluster_lamports: 100_000,
        node_stakes,
        validator_configs,
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let base_slot = 26; // S2
    let next_slot_on_a = 27; // S3
    let truncated_slots = 100; // just enough to purge all following slots after the S2 and S3

    let val_a_ledger_path = cluster.ledger_path(&validator_a_pubkey);
    let val_b_ledger_path = cluster.ledger_path(&validator_b_pubkey);
    let val_c_ledger_path = cluster.ledger_path(&validator_c_pubkey);

    info!(
        "val_a {} ledger path {:?}",
        validator_a_pubkey, val_a_ledger_path
    );
    info!(
        "val_b {} ledger path {:?}",
        validator_b_pubkey, val_b_ledger_path
    );
    info!(
        "val_c {} ledger path {:?}",
        validator_c_pubkey, val_c_ledger_path
    );

    // Immediately kill validator A, and C
    info!("Exiting validators A and C");
    let mut validator_a_info = cluster.exit_node(&validator_a_pubkey);
    let mut validator_c_info = cluster.exit_node(&validator_c_pubkey);

    // Step 1:
    // Let validator B, (D) run for a while.
    let now = Instant::now();
    loop {
        let elapsed = now.elapsed();
        assert!(
            elapsed <= Duration::from_secs(30),
            "Validator B failed to vote on any slot >= {} in {} secs",
            next_slot_on_a,
            elapsed.as_secs()
        );
        sleep(Duration::from_millis(100));

        if let Some((last_vote, _)) = last_vote_in_tower(&val_b_ledger_path, &validator_b_pubkey) {
            if last_vote >= next_slot_on_a {
                break;
            }
        }
    }
    // kill B
    let _validator_b_info = cluster.exit_node(&validator_b_pubkey);

    // Step 2:
    // Stop validator and truncate ledger, copy over B's ledger to A
    info!("truncate validator C's ledger");
    {
        // first copy from validator B's ledger
        std::fs::remove_dir_all(&validator_c_info.info.ledger_path).unwrap();
        let mut opt = fs_extra::dir::CopyOptions::new();
        opt.copy_inside = true;
        fs_extra::dir::copy(&val_b_ledger_path, &val_c_ledger_path, &opt).unwrap();
        // Remove B's tower in the C's new copied ledger
        remove_tower(&val_c_ledger_path, &validator_b_pubkey);

        let blockstore = open_blockstore(&val_c_ledger_path);
        purge_slots(&blockstore, base_slot + 1, truncated_slots);
    }
    info!("Create validator A's ledger");
    {
        // Find latest vote in B, and wait for it to reach blockstore
        let b_last_vote =
            wait_for_last_vote_in_tower_to_land_in_ledger(&val_b_ledger_path, &validator_b_pubkey);

        // Now we copy these blocks to A
        let b_blockstore = open_blockstore(&val_b_ledger_path);
        let a_blockstore = open_blockstore(&val_a_ledger_path);
        copy_blocks(b_last_vote, &b_blockstore, &a_blockstore);

        // Purge uneccessary slots
        purge_slots(&a_blockstore, next_slot_on_a + 1, truncated_slots);
    }

    // Step 3:
    // Restart A with voting enabled so that it can vote on B's fork
    // up to `next_slot_on_a`, thereby optimistcally confirming `next_slot_on_a`
    info!("Restarting A");
    validator_a_info.config.voting_disabled = false;
    cluster.restart_node(
        &validator_a_pubkey,
        validator_a_info,
        SocketAddrSpace::Unspecified,
    );

    info!("Waiting for A to vote on slot descended from slot `next_slot_on_a`");
    let now = Instant::now();
    loop {
        if let Some((last_vote_slot, _)) =
            last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey)
        {
            if last_vote_slot >= next_slot_on_a {
                info!(
                    "Validator A has caught up and voted on slot: {}",
                    last_vote_slot
                );
                break;
            }
        }

        if now.elapsed().as_secs() >= 30 {
            panic!(
                "Validator A has not seen optimistic confirmation slot > {} in 30 seconds",
                next_slot_on_a
            );
        }

        sleep(Duration::from_millis(20));
    }

    info!("Killing A");
    let validator_a_info = cluster.exit_node(&validator_a_pubkey);
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

    // Step 4:
    // Run validator C only to make it produce and vote on its own fork.
    info!("Restart validator C again!!!");
    validator_c_info.config.voting_disabled = false;
    cluster.restart_node(
        &validator_c_pubkey,
        validator_c_info,
        SocketAddrSpace::Unspecified,
    );

    let mut votes_on_c_fork = std::collections::BTreeSet::new(); // S4 and S5
    for _ in 0..100 {
        sleep(Duration::from_millis(100));

        if let Some((last_vote, _)) = last_vote_in_tower(&val_c_ledger_path, &validator_c_pubkey) {
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

    // Step 5:
    // verify whether there was violation or not
    info!("Restart validator A again!!!");
    cluster.restart_node(
        &validator_a_pubkey,
        validator_a_info,
        SocketAddrSpace::Unspecified,
    );

    // monitor for actual votes from validator A
    let mut bad_vote_detected = false;
    let mut a_votes = vec![];
    for _ in 0..100 {
        sleep(Duration::from_millis(100));

        if let Some((last_vote, _)) = last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey) {
            a_votes.push(last_vote);
            let blockstore = Blockstore::open_with_access_type(
                &val_a_ledger_path,
                AccessType::TryPrimaryThenSecondary,
                None,
                true,
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
    solana_logger::setup_with_default(RUST_LOG_FILTER);

    // First set up the cluster with 4 nodes
    let slots_per_epoch = 2048;
    let node_stakes = match cluster_mode {
        ClusterMode::MasterOnly => vec![100],
        ClusterMode::MasterSlave => vec![100, 1],
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
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default(),
            node_stakes.len(),
        ),
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

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

    cluster.restart_node(
        &validator_a_pubkey,
        validator_a_info,
        SocketAddrSpace::Unspecified,
    );

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
        let (last_vote, _) = last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey).unwrap();
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
    solana_logger::setup_with_default(RUST_LOG_FILTER);

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
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default(),
            node_stakes.len(),
        ),
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let cluster = std::sync::Arc::new(std::sync::Mutex::new(LocalCluster::new(
        &mut config,
        SocketAddrSpace::Unspecified,
    )));

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
        let restarted_validator_info = LocalCluster::restart_node_with_context(
            validator_a_info,
            restart_context,
            SocketAddrSpace::Unspecified,
        );
        cluster_for_a
            .lock()
            .unwrap()
            .add_node(&validator_a_pubkey, restarted_validator_info);
    });

    // test validator A actually to wait for supermajority
    let mut last_vote = None;
    for _ in 0..10 {
        sleep(Duration::from_millis(1000));

        let (new_last_vote, _) =
            last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey).unwrap();
        if let Some(last_vote) = last_vote {
            assert_eq!(last_vote, new_last_vote);
        } else {
            last_vote = Some(new_last_vote);
        }
    }

    // restart validator B normally
    cluster.lock().unwrap().restart_node(
        &validator_b_pubkey,
        validator_b_info,
        SocketAddrSpace::Unspecified,
    );

    // validator A should now start so join its thread here
    thread.join().unwrap();

    // new slots should be rooted after hard-fork cluster relaunch
    cluster
        .lock()
        .unwrap()
        .check_for_new_roots(16, "hard fork", SocketAddrSpace::Unspecified);
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
    run_test_load_program_accounts(CommitmentConfig::finalized());
}

#[test]
#[serial]
fn test_restart_tower_rollback() {
    // Test node crashing and failing to save its tower before restart
    solana_logger::setup_with_default(RUST_LOG_FILTER);

    // First set up the cluster with 4 nodes
    let slots_per_epoch = 2048;
    let node_stakes = vec![10000, 1];

    let validator_strings = vec![
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
        "2saHBBoTkLMmttmPQP8KfBkcCw45S5cwtV3wTdGCscRC8uxdgvHxpHiWXKx4LvJjNJtnNcbSv5NdheokFFqnNDt8",
    ];

    let validator_b_keypair = Arc::new(Keypair::from_base58_string(validator_strings[1]));
    let validator_b_pubkey = validator_b_keypair.pubkey();

    let validator_keys = validator_strings
        .iter()
        .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
        .take(node_stakes.len())
        .collect::<Vec<_>>();
    let mut config = ClusterConfig {
        cluster_lamports: 100_000,
        node_stakes: node_stakes.clone(),
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default(),
            node_stakes.len(),
        ),
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let val_b_ledger_path = cluster.ledger_path(&validator_b_pubkey);

    let mut earlier_tower: Tower;
    loop {
        sleep(Duration::from_millis(1000));

        // Grab the current saved tower
        earlier_tower = restore_tower(&val_b_ledger_path, &validator_b_pubkey).unwrap();
        if earlier_tower.last_voted_slot().unwrap_or(0) > 1 {
            break;
        }
    }

    let exited_validator_info: ClusterValidatorInfo;
    loop {
        sleep(Duration::from_millis(1000));

        // Wait for second, lesser staked validator to make a root past the earlier_tower's
        // latest vote slot, then exit that validator
        if let Some(root) = root_in_tower(&val_b_ledger_path, &validator_b_pubkey) {
            if root
                > earlier_tower
                    .last_voted_slot()
                    .expect("Earlier tower must have at least one vote")
            {
                exited_validator_info = cluster.exit_node(&validator_b_pubkey);
                break;
            }
        }
    }

    // Now rewrite the tower with the *earlier_tower*
    save_tower(&val_b_ledger_path, &earlier_tower, &validator_b_keypair);
    cluster.restart_node(
        &validator_b_pubkey,
        exited_validator_info,
        SocketAddrSpace::Unspecified,
    );

    // Check this node is making new roots
    cluster.check_for_new_roots(
        20,
        "test_restart_tower_rollback",
        SocketAddrSpace::Unspecified,
    );
}

#[test]
#[serial]
fn test_run_test_load_program_accounts_partition_root() {
    run_test_load_program_accounts_partition(CommitmentConfig::finalized());
}

fn run_test_load_program_accounts_partition(scan_commitment: CommitmentConfig) {
    let num_slots_per_validator = 8;
    let partitions: [Vec<usize>; 2] = [vec![1], vec![1]];
    let (leader_schedule, validator_keys) =
        create_custom_leader_schedule(&[num_slots_per_validator, num_slots_per_validator]);

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

    let on_partition_start = |cluster: &mut LocalCluster, _: &mut ()| {
        let update_client = cluster
            .get_validator_client(&cluster.entry_point_info.id)
            .unwrap();
        update_client_sender.send(update_client).unwrap();
        let scan_client = cluster
            .get_validator_client(&cluster.entry_point_info.id)
            .unwrap();
        scan_client_sender.send(scan_client).unwrap();
    };

    let on_partition_before_resolved = |_: &mut LocalCluster, _: &mut ()| {};

    let on_partition_resolved = |cluster: &mut LocalCluster, _: &mut ()| {
        cluster.check_for_new_roots(
            20,
            "run_test_load_program_accounts_partition",
            SocketAddrSpace::Unspecified,
        );
        exit.store(true, Ordering::Relaxed);
        t_update.join().unwrap();
        t_scan.join().unwrap();
    };

    run_cluster_partition(
        &partitions,
        Some((leader_schedule, validator_keys)),
        (),
        on_partition_start,
        on_partition_before_resolved,
        on_partition_resolved,
        None,
        None,
        additional_accounts,
    );
}

#[test]
#[serial]
fn test_votes_land_in_fork_during_long_partition() {
    let total_stake = 100;
    // Make `lighter_stake` insufficient for switching threshold
    let lighter_stake = (SWITCH_FORK_THRESHOLD as f64 * total_stake as f64) as u64;
    let heavier_stake = lighter_stake + 1;
    let failures_stake = total_stake - lighter_stake - heavier_stake;

    // Give lighter stake 30 consecutive slots before
    // the heavier stake gets a single slot
    let partitions: &[&[(usize, usize)]] = &[
        &[(heavier_stake as usize, 1)],
        &[(lighter_stake as usize, 30)],
    ];

    #[derive(Default)]
    struct PartitionContext {
        heaviest_validator_key: Pubkey,
        lighter_validator_key: Pubkey,
        heavier_fork_slot: Slot,
    }

    let on_partition_start = |_cluster: &mut LocalCluster,
                              validator_keys: &[Pubkey],
                              _dead_validator_infos: Vec<ClusterValidatorInfo>,
                              context: &mut PartitionContext| {
        // validator_keys[0] is the validator that will be killed, i.e. the validator with
        // stake == `failures_stake`
        context.heaviest_validator_key = validator_keys[1];
        context.lighter_validator_key = validator_keys[2];
    };

    let on_before_partition_resolved =
        |cluster: &mut LocalCluster, context: &mut PartitionContext| {
            let lighter_validator_ledger_path = cluster.ledger_path(&context.lighter_validator_key);
            let heavier_validator_ledger_path =
                cluster.ledger_path(&context.heaviest_validator_key);

            // Wait for each node to have created and voted on its own partition
            loop {
                let (heavier_validator_latest_vote_slot, _) = last_vote_in_tower(
                    &heavier_validator_ledger_path,
                    &context.heaviest_validator_key,
                )
                .unwrap();
                info!(
                    "Checking heavier validator's last vote {} is on a separate fork",
                    heavier_validator_latest_vote_slot
                );
                let lighter_validator_blockstore = open_blockstore(&lighter_validator_ledger_path);
                if lighter_validator_blockstore
                    .meta(heavier_validator_latest_vote_slot)
                    .unwrap()
                    .is_none()
                {
                    context.heavier_fork_slot = heavier_validator_latest_vote_slot;
                    return;
                }
                sleep(Duration::from_millis(100));
            }
        };

    let on_partition_resolved = |cluster: &mut LocalCluster, context: &mut PartitionContext| {
        let lighter_validator_ledger_path = cluster.ledger_path(&context.lighter_validator_key);
        let start = Instant::now();
        let max_wait = ms_for_n_slots(MAX_PROCESSING_AGE as u64, DEFAULT_TICKS_PER_SLOT);
        // Wait for the lighter node to switch over and root the `context.heavier_fork_slot`
        loop {
            assert!(
                // Should finish faster than if the cluster were relying on replay vote
                // refreshing to refresh the vote on blockhash expiration for the vote
                // transaction.
                !(start.elapsed() > Duration::from_millis(max_wait)),
                "Went too long {} ms without a root",
                max_wait,
            );
            let lighter_validator_blockstore = open_blockstore(&lighter_validator_ledger_path);
            if lighter_validator_blockstore.is_root(context.heavier_fork_slot) {
                info!(
                    "Partition resolved, new root made in {}ms",
                    start.elapsed().as_millis()
                );
                return;
            }
            sleep(Duration::from_millis(100));
        }
    };

    run_kill_partition_switch_threshold(
        &[&[(failures_stake as usize, 0)]],
        partitions,
        None,
        None,
        PartitionContext::default(),
        on_partition_start,
        on_before_partition_resolved,
        on_partition_resolved,
    );
}

fn setup_transfer_scan_threads(
    num_starting_accounts: usize,
    exit: Arc<AtomicBool>,
    scan_commitment: CommitmentConfig,
    update_client_receiver: Receiver<ThinClient>,
    scan_client_receiver: Receiver<ThinClient>,
) -> (
    JoinHandle<()>,
    JoinHandle<()>,
    Vec<(Pubkey, AccountSharedData)>,
) {
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
    let starting_accounts: Vec<(Pubkey, AccountSharedData)> = starting_keypairs
        .iter()
        .map(|k| {
            (
                k.pubkey(),
                AccountSharedData::new(1, 0, &system_program::id()),
            )
        })
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
                let (blockhash, _) = client
                    .get_latest_blockhash_with_commitment(CommitmentConfig::processed())
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
    solana_logger::setup_with_default(RUST_LOG_FILTER);
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
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default(),
            node_stakes.len(),
        ),
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        additional_accounts: starting_accounts,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

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
    cluster.check_for_new_roots(
        40,
        "run_test_load_program_accounts",
        SocketAddrSpace::Unspecified,
    );

    // Exit and ensure no violations of consistency were found
    exit.store(true, Ordering::Relaxed);
    t_update.join().unwrap();
    t_scan.join().unwrap();
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
    bank_snapshots_dir: TempDir,
    snapshot_archives_dir: TempDir,
    account_storage_dirs: Vec<TempDir>,
    validator_config: ValidatorConfig,
}

impl SnapshotValidatorConfig {
    pub fn new(
        full_snapshot_archive_interval_slots: Slot,
        incremental_snapshot_archive_interval_slots: Slot,
        accounts_hash_interval_slots: Slot,
        num_account_paths: usize,
    ) -> SnapshotValidatorConfig {
        assert!(accounts_hash_interval_slots > 0);
        assert!(full_snapshot_archive_interval_slots > 0);
        assert!(full_snapshot_archive_interval_slots % accounts_hash_interval_slots == 0);
        if incremental_snapshot_archive_interval_slots != Slot::MAX {
            assert!(incremental_snapshot_archive_interval_slots > 0);
            assert!(
                incremental_snapshot_archive_interval_slots % accounts_hash_interval_slots == 0
            );
            assert!(
                full_snapshot_archive_interval_slots % incremental_snapshot_archive_interval_slots
                    == 0
            );
        }

        // Create the snapshot config
        let bank_snapshots_dir = tempfile::tempdir_in(farf_dir()).unwrap();
        let snapshot_archives_dir = tempfile::tempdir_in(farf_dir()).unwrap();
        let snapshot_config = SnapshotConfig {
            full_snapshot_archive_interval_slots,
            incremental_snapshot_archive_interval_slots,
            snapshot_archives_dir: snapshot_archives_dir.path().to_path_buf(),
            bank_snapshots_dir: bank_snapshots_dir.path().to_path_buf(),
            ..SnapshotConfig::default()
        };

        // Create the account paths
        let (account_storage_dirs, account_storage_paths) =
            generate_account_paths(num_account_paths);

        // Create the validator config
        let validator_config = ValidatorConfig {
            snapshot_config: Some(snapshot_config),
            account_paths: account_storage_paths,
            accounts_hash_interval_slots,
            ..ValidatorConfig::default()
        };

        SnapshotValidatorConfig {
            bank_snapshots_dir,
            snapshot_archives_dir,
            account_storage_dirs,
            validator_config,
        }
    }
}

fn setup_snapshot_validator_config(
    snapshot_interval_slots: Slot,
    num_account_paths: usize,
) -> SnapshotValidatorConfig {
    SnapshotValidatorConfig::new(
        snapshot_interval_slots,
        Slot::MAX,
        snapshot_interval_slots,
        num_account_paths,
    )
}

/// Computes the numbr of milliseconds `num_blocks` blocks will take given
/// each slot contains `ticks_per_slot`
fn ms_for_n_slots(num_blocks: u64, ticks_per_slot: u64) -> u64 {
    ((ticks_per_slot * DEFAULT_MS_PER_SLOT * num_blocks) + DEFAULT_TICKS_PER_SLOT - 1)
        / DEFAULT_TICKS_PER_SLOT
}

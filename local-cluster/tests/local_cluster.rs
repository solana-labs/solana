#![allow(clippy::arithmetic_side_effects)]
use {
    assert_matches::assert_matches,
    crossbeam_channel::{unbounded, Receiver},
    gag::BufferRedirect,
    log::*,
    serial_test::serial,
    solana_accounts_db::{
        accounts_db::create_accounts_run_and_snapshot_dirs, hardened_unpack::open_genesis_config,
    },
    solana_client::thin_client::ThinClient,
    solana_core::{
        consensus::{
            tower_storage::FileTowerStorage, Tower, SWITCH_FORK_THRESHOLD, VOTE_THRESHOLD_DEPTH,
        },
        optimistic_confirmation_verifier::OptimisticConfirmationVerifier,
        replay_stage::DUPLICATE_THRESHOLD,
        validator::ValidatorConfig,
    },
    solana_download_utils::download_snapshot_archive,
    solana_entry::entry::create_ticks,
    solana_gossip::{contact_info::LegacyContactInfo, gossip_service::discover_cluster},
    solana_ledger::{
        ancestor_iterator::AncestorIterator,
        bank_forks_utils,
        blockstore::{entries_to_test_shreds, Blockstore},
        blockstore_processor::ProcessOptions,
        leader_schedule::FixedSchedule,
        shred::{ProcessShredsStats, ReedSolomonCache, Shred, Shredder},
        use_snapshot_archives_at_startup::UseSnapshotArchivesAtStartup,
    },
    solana_local_cluster::{
        cluster::{Cluster, ClusterValidatorInfo},
        cluster_tests,
        integration_tests::{
            copy_blocks, create_custom_leader_schedule,
            create_custom_leader_schedule_with_random_keys, farf_dir, generate_account_paths,
            last_root_in_tower, last_vote_in_tower, ms_for_n_slots, open_blockstore,
            purge_slots_with_count, remove_tower, remove_tower_if_exists, restore_tower,
            run_cluster_partition, run_kill_partition_switch_threshold, save_tower,
            setup_snapshot_validator_config, test_faulty_node, wait_for_duplicate_proof,
            wait_for_last_vote_in_tower_to_land_in_ledger, SnapshotValidatorConfig,
            ValidatorTestConfig, DEFAULT_CLUSTER_LAMPORTS, DEFAULT_NODE_STAKE, RUST_LOG_FILTER,
        },
        local_cluster::{ClusterConfig, LocalCluster},
        validator_configs::*,
    },
    solana_pubsub_client::pubsub_client::PubsubClient,
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_api::{
        config::{
            RpcBlockSubscribeConfig, RpcBlockSubscribeFilter, RpcProgramAccountsConfig,
            RpcSignatureSubscribeConfig,
        },
        response::RpcSignatureResult,
    },
    solana_runtime::{
        commitment::VOTE_THRESHOLD_SIZE,
        snapshot_archive_info::SnapshotArchiveInfoGetter,
        snapshot_bank_utils,
        snapshot_config::SnapshotConfig,
        snapshot_package::SnapshotKind,
        snapshot_utils::{self},
    },
    solana_sdk::{
        account::AccountSharedData,
        client::{AsyncClient, SyncClient},
        clock::{self, Slot, DEFAULT_TICKS_PER_SLOT, MAX_PROCESSING_AGE},
        commitment_config::CommitmentConfig,
        epoch_schedule::{DEFAULT_SLOTS_PER_EPOCH, MINIMUM_SLOTS_PER_EPOCH},
        genesis_config::ClusterType,
        hard_forks::HardForks,
        hash::Hash,
        poh_config::PohConfig,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_program, system_transaction,
        vote::state::VoteStateUpdate,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_turbine::broadcast_stage::{
        broadcast_duplicates_run::{BroadcastDuplicatesConfig, ClusterPartition},
        BroadcastStageType,
    },
    solana_vote::vote_parser,
    solana_vote_program::{vote_state::MAX_LOCKOUT_HISTORY, vote_transaction},
    std::{
        collections::{BTreeSet, HashMap, HashSet},
        fs,
        io::Read,
        iter,
        num::NonZeroUsize,
        path::Path,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Mutex,
        },
        thread::{sleep, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

#[test]
fn test_local_cluster_start_and_exit() {
    solana_logger::setup();
    let num_nodes = 1;
    let cluster = LocalCluster::new_with_equal_stakes(
        num_nodes,
        DEFAULT_CLUSTER_LAMPORTS,
        DEFAULT_NODE_STAKE,
        SocketAddrSpace::Unspecified,
    );
    assert_eq!(cluster.validators.len(), num_nodes);
}

#[test]
fn test_local_cluster_start_and_exit_with_config() {
    solana_logger::setup();
    const NUM_NODES: usize = 1;
    let mut config = ClusterConfig {
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default_for_test(),
            NUM_NODES,
        ),
        node_stakes: vec![DEFAULT_NODE_STAKE; NUM_NODES],
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        ticks_per_slot: 8,
        slots_per_epoch: MINIMUM_SLOTS_PER_EPOCH,
        stakers_slot_offset: MINIMUM_SLOTS_PER_EPOCH,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
    assert_eq!(cluster.validators.len(), NUM_NODES);
}

#[test]
#[serial]
fn test_spend_and_verify_all_nodes_1() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_spend_and_verify_all_nodes_1");
    let num_nodes = 1;
    let local = LocalCluster::new_with_equal_stakes(
        num_nodes,
        DEFAULT_CLUSTER_LAMPORTS,
        DEFAULT_NODE_STAKE,
        SocketAddrSpace::Unspecified,
    );
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
        HashSet::new(),
        SocketAddrSpace::Unspecified,
        &local.connection_cache,
    );
}

#[test]
#[serial]
fn test_spend_and_verify_all_nodes_2() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_spend_and_verify_all_nodes_2");
    let num_nodes = 2;
    let local = LocalCluster::new_with_equal_stakes(
        num_nodes,
        DEFAULT_CLUSTER_LAMPORTS,
        DEFAULT_NODE_STAKE,
        SocketAddrSpace::Unspecified,
    );
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
        HashSet::new(),
        SocketAddrSpace::Unspecified,
        &local.connection_cache,
    );
}

#[test]
#[serial]
fn test_spend_and_verify_all_nodes_3() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_spend_and_verify_all_nodes_3");
    let num_nodes = 3;
    let local = LocalCluster::new_with_equal_stakes(
        num_nodes,
        DEFAULT_CLUSTER_LAMPORTS,
        DEFAULT_NODE_STAKE,
        SocketAddrSpace::Unspecified,
    );
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
        HashSet::new(),
        SocketAddrSpace::Unspecified,
        &local.connection_cache,
    );
}

#[test]
#[serial]
#[ignore]
fn test_local_cluster_signature_subscribe() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let num_nodes = 2;
    let cluster = LocalCluster::new_with_equal_stakes(
        num_nodes,
        DEFAULT_CLUSTER_LAMPORTS,
        DEFAULT_NODE_STAKE,
        SocketAddrSpace::Unspecified,
    );
    let nodes = cluster.get_node_pubkeys();

    // Get non leader
    let non_bootstrap_id = nodes
        .into_iter()
        .find(|id| id != cluster.entry_point_info.pubkey())
        .unwrap();
    let non_bootstrap_info = cluster.get_contact_info(&non_bootstrap_id).unwrap();

    let (rpc, tpu) = LegacyContactInfo::try_from(non_bootstrap_info)
        .map(|node| {
            cluster_tests::get_client_facing_addr(cluster.connection_cache.protocol(), node)
        })
        .unwrap();
    let tx_client = ThinClient::new(rpc, tpu, cluster.connection_cache.clone());

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
        &format!(
            "ws://{}",
            &non_bootstrap_info.rpc_pubsub().unwrap().to_string()
        ),
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
    let local = LocalCluster::new_with_equal_stakes(
        num_nodes,
        DEFAULT_CLUSTER_LAMPORTS,
        DEFAULT_NODE_STAKE,
        SocketAddrSpace::Unspecified,
    );
    cluster_tests::spend_and_verify_all_nodes(
        &local.entry_point_info,
        &local.funding_keypair,
        num_nodes,
        HashSet::new(),
        SocketAddrSpace::Unspecified,
        &local.connection_cache,
    );
}

#[test]
#[serial]
fn test_two_unbalanced_stakes() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_two_unbalanced_stakes");
    let validator_config = ValidatorConfig::default_for_test();
    let num_ticks_per_second = 100;
    let num_ticks_per_slot = 10;
    let num_slots_per_epoch = MINIMUM_SLOTS_PER_EPOCH;

    let mut cluster = LocalCluster::new(
        &mut ClusterConfig {
            node_stakes: vec![DEFAULT_NODE_STAKE * 100, DEFAULT_NODE_STAKE],
            cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + DEFAULT_NODE_STAKE * 100,
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
    let leader_pubkey = *cluster.entry_point_info.pubkey();
    let leader_ledger = cluster.validators[&leader_pubkey].info.ledger_path.clone();
    cluster_tests::verify_ledger_ticks(&leader_ledger, num_ticks_per_slot as usize);
}

#[test]
#[serial]
fn test_forwarding() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    // Set up a cluster where one node is never the leader, so all txs sent to this node
    // will be have to be forwarded in order to be confirmed
    let mut config = ClusterConfig {
        node_stakes: vec![DEFAULT_NODE_STAKE * 100, DEFAULT_NODE_STAKE],
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + DEFAULT_NODE_STAKE * 100,
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default_for_test(),
            2,
        ),
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let cluster_nodes = discover_cluster(
        &cluster.entry_point_info.gossip().unwrap(),
        2,
        SocketAddrSpace::Unspecified,
    )
    .unwrap();
    assert!(cluster_nodes.len() >= 2);

    let leader_pubkey = *cluster.entry_point_info.pubkey();

    let validator_info = cluster_nodes
        .iter()
        .find(|c| c.pubkey() != &leader_pubkey)
        .unwrap();

    // Confirm that transactions were forwarded to and processed by the leader.
    cluster_tests::send_many_transactions(
        validator_info,
        &cluster.funding_keypair,
        &cluster.connection_cache,
        10,
        20,
    );
}

#[test]
#[serial]
fn test_restart_node() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_restart_node");
    let slots_per_epoch = MINIMUM_SLOTS_PER_EPOCH * 2;
    let ticks_per_slot = 16;
    let validator_config = ValidatorConfig::default_for_test();
    let mut cluster = LocalCluster::new(
        &mut ClusterConfig {
            node_stakes: vec![DEFAULT_NODE_STAKE],
            cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
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
        &LegacyContactInfo::try_from(&cluster.entry_point_info).unwrap(),
        &cluster.funding_keypair,
        &cluster.connection_cache,
        10,
        1,
    );
}

#[test]
#[serial]
fn test_mainnet_beta_cluster_type() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);

    let mut config = ClusterConfig {
        cluster_type: ClusterType::MainnetBeta,
        node_stakes: vec![DEFAULT_NODE_STAKE],
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default_for_test(),
            1,
        ),
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
    let cluster_nodes = discover_cluster(
        &cluster.entry_point_info.gossip().unwrap(),
        1,
        SocketAddrSpace::Unspecified,
    )
    .unwrap();
    assert_eq!(cluster_nodes.len(), 1);

    let (rpc, tpu) = LegacyContactInfo::try_from(&cluster.entry_point_info)
        .map(|node| {
            cluster_tests::get_client_facing_addr(cluster.connection_cache.protocol(), node)
        })
        .unwrap();
    let client = ThinClient::new(rpc, tpu, cluster.connection_cache.clone());

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
fn test_snapshot_download() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    // First set up the cluster with 1 node
    let snapshot_interval_slots = 50;
    let num_account_paths = 3;

    let leader_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
    let validator_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);

    let stake = DEFAULT_NODE_STAKE;
    let mut config = ClusterConfig {
        node_stakes: vec![stake],
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        validator_configs: make_identical_validator_configs(
            &leader_snapshot_test_config.validator_config,
            1,
        ),
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let full_snapshot_archives_dir = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .full_snapshot_archives_dir;

    trace!("Waiting for snapshot");
    let full_snapshot_archive_info = cluster.wait_for_next_full_snapshot(
        full_snapshot_archives_dir,
        Some(Duration::from_secs(5 * 60)),
    );
    trace!("found: {}", full_snapshot_archive_info.path().display());

    // Download the snapshot, then boot a validator from it.
    download_snapshot_archive(
        &cluster.entry_point_info.rpc().unwrap(),
        &validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .full_snapshot_archives_dir,
        &validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .incremental_snapshot_archives_dir,
        (
            full_snapshot_archive_info.slot(),
            *full_snapshot_archive_info.hash(),
        ),
        SnapshotKind::FullSnapshot,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .maximum_full_snapshot_archives_to_retain,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
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

    let stake = DEFAULT_NODE_STAKE;
    let mut config = ClusterConfig {
        node_stakes: vec![stake],
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        validator_configs: make_identical_validator_configs(
            &leader_snapshot_test_config.validator_config,
            1,
        ),
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let full_snapshot_archives_dir = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .full_snapshot_archives_dir;
    let incremental_snapshot_archives_dir = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .incremental_snapshot_archives_dir;

    debug!("snapshot config:\n\tfull snapshot interval: {}\n\tincremental snapshot interval: {}\n\taccounts hash interval: {}",
           full_snapshot_interval,
           incremental_snapshot_interval,
           accounts_hash_interval);
    debug!(
        "leader config:\n\tbank snapshots dir: {}\n\tfull snapshot archives dir: {}\n\tincremental snapshot archives dir: {}",
        leader_snapshot_test_config
            .bank_snapshots_dir
            .path()
            .display(),
        leader_snapshot_test_config
            .full_snapshot_archives_dir
            .path()
            .display(),
        leader_snapshot_test_config
            .incremental_snapshot_archives_dir
            .path()
            .display(),
    );
    debug!(
        "validator config:\n\tbank snapshots dir: {}\n\tfull snapshot archives dir: {}\n\tincremental snapshot archives dir: {}",
        validator_snapshot_test_config
            .bank_snapshots_dir
            .path()
            .display(),
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .path()
            .display(),
        validator_snapshot_test_config
            .incremental_snapshot_archives_dir
            .path()
            .display(),
    );

    trace!("Waiting for snapshots");
    let (incremental_snapshot_archive_info, full_snapshot_archive_info) = cluster
        .wait_for_next_incremental_snapshot(
            full_snapshot_archives_dir,
            incremental_snapshot_archives_dir,
            Some(Duration::from_secs(5 * 60)),
        );
    trace!(
        "found: {} and {}",
        full_snapshot_archive_info.path().display(),
        incremental_snapshot_archive_info.path().display()
    );

    // Download the snapshots, then boot a validator from them.
    download_snapshot_archive(
        &cluster.entry_point_info.rpc().unwrap(),
        &validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .full_snapshot_archives_dir,
        &validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .incremental_snapshot_archives_dir,
        (
            full_snapshot_archive_info.slot(),
            *full_snapshot_archive_info.hash(),
        ),
        SnapshotKind::FullSnapshot,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .maximum_full_snapshot_archives_to_retain,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .maximum_incremental_snapshot_archives_to_retain,
        false,
        &mut None,
    )
    .unwrap();

    download_snapshot_archive(
        &cluster.entry_point_info.rpc().unwrap(),
        &validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .full_snapshot_archives_dir,
        &validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .incremental_snapshot_archives_dir,
        (
            incremental_snapshot_archive_info.slot(),
            *incremental_snapshot_archive_info.hash(),
        ),
        SnapshotKind::IncrementalSnapshot(incremental_snapshot_archive_info.base_slot()),
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .maximum_full_snapshot_archives_to_retain,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
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
    let full_snapshot_interval = incremental_snapshot_interval * 5;

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
    let stake = DEFAULT_NODE_STAKE;
    let mut config = ClusterConfig {
        node_stakes: vec![stake],
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        validator_configs: make_identical_validator_configs(
            &leader_snapshot_test_config.validator_config,
            1,
        ),
        ..ClusterConfig::default()
    };

    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    info!("snapshot config:\n\tfull snapshot interval: {}\n\tincremental snapshot interval: {}\n\taccounts hash interval: {}",
           full_snapshot_interval,
           incremental_snapshot_interval,
           accounts_hash_interval);
    debug!(
        "leader config:\n\tbank snapshots dir: {}\n\tfull snapshot archives dir: {}\n\tincremental snapshot archives dir: {}",
        leader_snapshot_test_config
            .bank_snapshots_dir
            .path()
            .display(),
        leader_snapshot_test_config
            .full_snapshot_archives_dir
            .path()
            .display(),
        leader_snapshot_test_config
            .incremental_snapshot_archives_dir
            .path()
            .display(),
    );
    debug!(
        "validator config:\n\tbank snapshots dir: {}\n\tfull snapshot archives dir: {}\n\tincremental snapshot archives dir: {}",
        validator_snapshot_test_config
            .bank_snapshots_dir
            .path()
            .display(),
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .path()
            .display(),
        validator_snapshot_test_config
            .incremental_snapshot_archives_dir
            .path()
            .display(),
    );

    info!("Waiting for leader to create the next incremental snapshot...");
    let (incremental_snapshot_archive, full_snapshot_archive) =
        LocalCluster::wait_for_next_incremental_snapshot(
            &cluster,
            leader_snapshot_test_config
                .full_snapshot_archives_dir
                .path(),
            leader_snapshot_test_config
                .incremental_snapshot_archives_dir
                .path(),
            Some(Duration::from_secs(5 * 60)),
        );
    info!(
        "Found snapshots:\n\tfull snapshot: {}\n\tincremental snapshot: {}",
        full_snapshot_archive.path().display(),
        incremental_snapshot_archive.path().display()
    );
    assert_eq!(
        full_snapshot_archive.slot(),
        incremental_snapshot_archive.base_slot()
    );
    info!("Waiting for leader to create snapshots... DONE");

    // Download the snapshots, then boot a validator from them.
    info!("Downloading full snapshot to validator...");
    download_snapshot_archive(
        &cluster.entry_point_info.rpc().unwrap(),
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .path(),
        validator_snapshot_test_config
            .incremental_snapshot_archives_dir
            .path(),
        (full_snapshot_archive.slot(), *full_snapshot_archive.hash()),
        SnapshotKind::FullSnapshot,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .maximum_full_snapshot_archives_to_retain,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .maximum_incremental_snapshot_archives_to_retain,
        false,
        &mut None,
    )
    .unwrap();
    let downloaded_full_snapshot_archive = snapshot_utils::get_highest_full_snapshot_archive_info(
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .path(),
    )
    .unwrap();
    info!(
        "Downloaded full snapshot, slot: {}",
        downloaded_full_snapshot_archive.slot()
    );

    info!("Downloading incremental snapshot to validator...");
    download_snapshot_archive(
        &cluster.entry_point_info.rpc().unwrap(),
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .path(),
        validator_snapshot_test_config
            .incremental_snapshot_archives_dir
            .path(),
        (
            incremental_snapshot_archive.slot(),
            *incremental_snapshot_archive.hash(),
        ),
        SnapshotKind::IncrementalSnapshot(incremental_snapshot_archive.base_slot()),
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .maximum_full_snapshot_archives_to_retain,
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .maximum_incremental_snapshot_archives_to_retain,
        false,
        &mut None,
    )
    .unwrap();
    let downloaded_incremental_snapshot_archive =
        snapshot_utils::get_highest_incremental_snapshot_archive_info(
            validator_snapshot_test_config
                .incremental_snapshot_archives_dir
                .path(),
            full_snapshot_archive.slot(),
        )
        .unwrap();
    info!(
        "Downloaded incremental snapshot, slot: {}, base slot: {}",
        downloaded_incremental_snapshot_archive.slot(),
        downloaded_incremental_snapshot_archive.base_slot(),
    );
    assert_eq!(
        downloaded_full_snapshot_archive.slot(),
        downloaded_incremental_snapshot_archive.base_slot()
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

    let copy_files_with_remote = |from: &Path, to: &Path| {
        copy_files(from, to);
        let remote_from = snapshot_utils::build_snapshot_archives_remote_dir(from);
        let remote_to = snapshot_utils::build_snapshot_archives_remote_dir(to);
        let _ = fs::create_dir_all(&remote_from);
        let _ = fs::create_dir_all(&remote_to);
        copy_files(&remote_from, &remote_to);
    };

    let delete_files_with_remote = |from: &Path| {
        delete_files(from);
        let remote_dir = snapshot_utils::build_snapshot_archives_remote_dir(from);
        let _ = fs::create_dir_all(&remote_dir);
        delete_files(&remote_dir);
    };

    // After downloading the snapshots, copy them over to a backup directory.  Later we'll need to
    // restart the node and guarantee that the only snapshots present are these initial ones.  So,
    // the easiest way to do that is create a backup now, delete the ones on the node before
    // restart, then copy the backup ones over again.
    let backup_validator_full_snapshot_archives_dir = tempfile::tempdir_in(farf_dir()).unwrap();
    trace!(
        "Backing up validator full snapshots to dir: {}...",
        backup_validator_full_snapshot_archives_dir.path().display()
    );
    copy_files_with_remote(
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .path(),
        backup_validator_full_snapshot_archives_dir.path(),
    );
    let backup_validator_incremental_snapshot_archives_dir =
        tempfile::tempdir_in(farf_dir()).unwrap();
    trace!(
        "Backing up validator incremental snapshots to dir: {}...",
        backup_validator_incremental_snapshot_archives_dir
            .path()
            .display()
    );
    copy_files_with_remote(
        validator_snapshot_test_config
            .incremental_snapshot_archives_dir
            .path(),
        backup_validator_incremental_snapshot_archives_dir.path(),
    );

    info!("Starting the validator...");
    let validator_identity = Arc::new(Keypair::new());
    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        stake,
        validator_identity.clone(),
        None,
        SocketAddrSpace::Unspecified,
    );
    info!("Starting the validator... DONE");

    // To ensure that a snapshot will be taken during startup, the blockstore needs to have roots
    // that cross a full snapshot interval.
    let starting_slot = incremental_snapshot_archive.slot();
    let next_full_snapshot_slot = starting_slot + full_snapshot_interval;
    info!("Waiting for the validator to see enough slots to cross a full snapshot interval ({next_full_snapshot_slot})...");
    let timer = Instant::now();
    loop {
        let validator_current_slot = cluster
            .get_validator_client(&validator_identity.pubkey())
            .unwrap()
            .get_slot_with_commitment(CommitmentConfig::finalized())
            .unwrap();
        trace!("validator current slot: {validator_current_slot}");
        if validator_current_slot > next_full_snapshot_slot {
            break;
        }
        assert!(
            timer.elapsed() < Duration::from_secs(30),
            "It should not take longer than 30 seconds to cross the next full snapshot interval."
        );
        std::thread::yield_now();
    }
    info!(
        "Waited {:?} for the validator to see enough slots to cross a full snapshot interval... DONE", timer.elapsed()
    );

    // Get the highest full snapshot archive info for the validator, now that it has crossed the
    // next full snapshot interval.  We are going to use this to look up the same snapshot on the
    // leader, which we'll then use to compare to the full snapshot the validator will create
    // during startup.  This ensures the snapshot creation process during startup is correct.
    //
    // Putting this all in its own block so its clear we're only intended to keep the leader's info
    let leader_full_snapshot_archive_for_comparison = {
        let validator_full_snapshot = snapshot_utils::get_highest_full_snapshot_archive_info(
            validator_snapshot_test_config
                .full_snapshot_archives_dir
                .path(),
        )
        .unwrap();

        // Now get the same full snapshot on the LEADER that we just got from the validator
        let mut leader_full_snapshots = snapshot_utils::get_full_snapshot_archives(
            leader_snapshot_test_config
                .full_snapshot_archives_dir
                .path(),
        );
        leader_full_snapshots.retain(|full_snapshot| {
            full_snapshot.slot() == validator_full_snapshot.slot()
                && full_snapshot.hash() == validator_full_snapshot.hash()
        });
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
    info!("leader full snapshot archive for comparison: {leader_full_snapshot_archive_for_comparison:#?}");

    // Stop the validator before we reset its snapshots
    info!("Stopping the validator...");
    let validator_info = cluster.exit_node(&validator_identity.pubkey());
    info!("Stopping the validator... DONE");

    info!("Delete all the snapshots on the validator and restore the originals from the backup...");
    delete_files_with_remote(
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .path(),
    );
    delete_files_with_remote(
        validator_snapshot_test_config
            .incremental_snapshot_archives_dir
            .path(),
    );
    copy_files_with_remote(
        backup_validator_full_snapshot_archives_dir.path(),
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .path(),
    );
    copy_files_with_remote(
        backup_validator_incremental_snapshot_archives_dir.path(),
        validator_snapshot_test_config
            .incremental_snapshot_archives_dir
            .path(),
    );
    info!(
        "Delete all the snapshots on the validator and restore the originals from the backup... DONE"
    );

    // Get the highest full snapshot slot *before* restarting, as a comparison
    let validator_full_snapshot_slot_at_startup =
        snapshot_utils::get_highest_full_snapshot_archive_slot(
            validator_snapshot_test_config
                .full_snapshot_archives_dir
                .path(),
        )
        .unwrap();

    info!(
        "Restarting the validator with full snapshot {validator_full_snapshot_slot_at_startup}..."
    );
    cluster.restart_node(
        &validator_identity.pubkey(),
        validator_info,
        SocketAddrSpace::Unspecified,
    );
    info!("Restarting the validator... DONE");

    // Now, we want to ensure that the validator can make a new incremental snapshot based on the
    // new full snapshot that was created during the restart.
    info!("Waiting for the validator to make new snapshots...");
    let validator_next_full_snapshot_slot =
        validator_full_snapshot_slot_at_startup + full_snapshot_interval;
    let validator_next_incremental_snapshot_slot =
        validator_next_full_snapshot_slot + incremental_snapshot_interval;
    info!("Waiting for validator next full snapshot slot: {validator_next_full_snapshot_slot}");
    info!("Waiting for validator next incremental snapshot slot: {validator_next_incremental_snapshot_slot}");
    let timer = Instant::now();
    loop {
        if let Some(full_snapshot_slot) = snapshot_utils::get_highest_full_snapshot_archive_slot(
            validator_snapshot_test_config
                .full_snapshot_archives_dir
                .path(),
        ) {
            if full_snapshot_slot >= validator_next_full_snapshot_slot {
                if let Some(incremental_snapshot_slot) =
                    snapshot_utils::get_highest_incremental_snapshot_archive_slot(
                        validator_snapshot_test_config
                            .incremental_snapshot_archives_dir
                            .path(),
                        full_snapshot_slot,
                    )
                {
                    if incremental_snapshot_slot >= validator_next_incremental_snapshot_slot {
                        // specific incremental snapshot is not important, just that one was created
                        info!(
                             "Validator made new snapshots, full snapshot slot: {}, incremental snapshot slot: {}",
                             full_snapshot_slot,
                             incremental_snapshot_slot,
                        );
                        break;
                    }
                }
            }
        }

        assert!(
            timer.elapsed() < Duration::from_secs(30),
            "It should not take longer than 30 seconds to cross the next incremental snapshot interval."
        );
        std::thread::yield_now();
    }
    info!(
        "Waited {:?} for the validator to make new snapshots... DONE",
        timer.elapsed()
    );

    // Check to make sure that the full snapshot the validator created during startup is the same
    // or one greater than the snapshot the leader created.
    let validator_full_snapshot_archives = snapshot_utils::get_full_snapshot_archives(
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .path(),
    );
    info!("validator full snapshot archives: {validator_full_snapshot_archives:#?}");
    let validator_full_snapshot_archive_for_comparison = validator_full_snapshot_archives
        .into_iter()
        .find(|validator_full_snapshot_archive| {
            validator_full_snapshot_archive.slot()
                == leader_full_snapshot_archive_for_comparison.slot()
        })
        .expect("validator created an unexpected full snapshot");
    info!("Validator full snapshot archive for comparison: {validator_full_snapshot_archive_for_comparison:#?}");
    assert_eq!(
        validator_full_snapshot_archive_for_comparison.hash(),
        leader_full_snapshot_archive_for_comparison.hash(),
    );

    // And lastly, startup another node with the new snapshots to ensure they work
    let final_validator_snapshot_test_config = SnapshotValidatorConfig::new(
        full_snapshot_interval,
        incremental_snapshot_interval,
        accounts_hash_interval,
        num_account_paths,
    );

    // Copy over the snapshots to the new node that it will boot from
    copy_files(
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .path(),
        final_validator_snapshot_test_config
            .full_snapshot_archives_dir
            .path(),
    );
    copy_files(
        validator_snapshot_test_config
            .incremental_snapshot_archives_dir
            .path(),
        final_validator_snapshot_test_config
            .incremental_snapshot_archives_dir
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
    info!("Starting final validator... DONE");
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
        node_stakes: vec![DEFAULT_NODE_STAKE * 100, DEFAULT_NODE_STAKE],
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + DEFAULT_NODE_STAKE * 100,
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
        .find(|x| x != cluster.entry_point_info.pubkey())
        .unwrap();
    let validator_info = cluster.exit_node(&validator_id);

    // Get slot after which this was generated
    let full_snapshot_archives_dir = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .full_snapshot_archives_dir;

    let full_snapshot_archive_info = cluster.wait_for_next_full_snapshot(
        full_snapshot_archives_dir,
        Some(Duration::from_secs(5 * 60)),
    );

    // Copy archive to validator's snapshot output directory
    let validator_archive_path = snapshot_utils::build_full_snapshot_archive_path(
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .into_path(),
        full_snapshot_archive_info.slot(),
        full_snapshot_archive_info.hash(),
        full_snapshot_archive_info.archive_format(),
    );
    fs::hard_link(full_snapshot_archive_info.path(), validator_archive_path).unwrap();

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
        &cluster.connection_cache,
    );
}

#[test]
#[serial]
fn test_snapshots_blockstore_floor() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    // First set up the cluster with 1 snapshotting leader
    let snapshot_interval_slots = 100;
    let num_account_paths = 4;

    let leader_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
    let mut validator_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);

    let full_snapshot_archives_dir = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .full_snapshot_archives_dir;

    let mut config = ClusterConfig {
        node_stakes: vec![DEFAULT_NODE_STAKE],
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
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
            snapshot_utils::get_highest_full_snapshot_archive_info(full_snapshot_archives_dir);
        if archive.is_some() {
            trace!("snapshot exists");
            break archive.unwrap();
        }
        sleep(Duration::from_millis(5000));
    };

    // Copy archive to validator's snapshot output directory
    let validator_archive_path = snapshot_utils::build_full_snapshot_archive_path(
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .into_path(),
        archive_info.slot(),
        archive_info.hash(),
        validator_snapshot_test_config
            .validator_config
            .snapshot_config
            .archive_format,
    );
    fs::hard_link(archive_info.path(), validator_archive_path).unwrap();
    let slot_floor = archive_info.slot();

    // Start up a new node from a snapshot
    let cluster_nodes = discover_cluster(
        &cluster.entry_point_info.gossip().unwrap(),
        1,
        SocketAddrSpace::Unspecified,
    )
    .unwrap();
    let mut known_validators = HashSet::new();
    known_validators.insert(*cluster_nodes[0].pubkey());
    validator_snapshot_test_config
        .validator_config
        .known_validators = Some(known_validators);

    cluster.add_validator(
        &validator_snapshot_test_config.validator_config,
        DEFAULT_NODE_STAKE,
        Arc::new(Keypair::new()),
        None,
        SocketAddrSpace::Unspecified,
    );
    let all_pubkeys = cluster.get_node_pubkeys();
    let validator_id = all_pubkeys
        .into_iter()
        .find(|x| x != cluster.entry_point_info.pubkey())
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
    let snapshot_interval_slots = 100;
    let num_account_paths = 1;
    let mut snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
    let full_snapshot_archives_dir = &snapshot_test_config
        .validator_config
        .snapshot_config
        .full_snapshot_archives_dir;

    // Set up the cluster with 1 snapshotting validator
    let mut all_account_storage_dirs = vec![std::mem::take(
        &mut snapshot_test_config.account_storage_dirs,
    )];
    let mut config = ClusterConfig {
        node_stakes: vec![DEFAULT_NODE_STAKE],
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
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
            &LegacyContactInfo::try_from(&cluster.entry_point_info).unwrap(),
            &cluster.funding_keypair,
            &cluster.connection_cache,
            10,
            10,
        );

        expected_balances.extend(new_balances);

        cluster.wait_for_next_full_snapshot(
            full_snapshot_archives_dir,
            Some(Duration::from_secs(5 * 60)),
        );

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
        cluster_tests::verify_balances(
            expected_balances.clone(),
            &cluster.entry_point_info,
            cluster.connection_cache.clone(),
        );

        // Check that we can still push transactions
        trace!("Spending and verifying");
        cluster_tests::spend_and_verify_all_nodes(
            &cluster.entry_point_info,
            &cluster.funding_keypair,
            1,
            HashSet::new(),
            SocketAddrSpace::Unspecified,
            &cluster.connection_cache,
        );
    }
}

#[test]
#[serial]
#[allow(unused_attributes)]
#[ignore]
fn test_fail_entry_verification_leader() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let leader_stake = (DUPLICATE_THRESHOLD * 100.0) as u64 + 1;
    let validator_stake1 = (100 - leader_stake) / 2;
    let validator_stake2 = 100 - leader_stake - validator_stake1;
    let (cluster, _) = test_faulty_node(
        BroadcastStageType::FailEntryVerification,
        vec![leader_stake, validator_stake1, validator_stake2],
        None,
        None,
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
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let node_stakes = vec![300, 100];
    let (cluster, _) = test_faulty_node(
        BroadcastStageType::BroadcastFakeShreds,
        node_stakes,
        None,
        None,
    );
    cluster.check_for_new_roots(
        16,
        "test_fake_shreds_broadcast_leader",
        SocketAddrSpace::Unspecified,
    );
}

#[test]
fn test_wait_for_max_stake() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let validator_config = ValidatorConfig::default_for_test();
    let slots_per_epoch = MINIMUM_SLOTS_PER_EPOCH;
    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        node_stakes: vec![DEFAULT_NODE_STAKE; 4],
        validator_configs: make_identical_validator_configs(&validator_config, 4),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
    let client = RpcClient::new_socket(cluster.entry_point_info.rpc().unwrap());

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
        ..ValidatorConfig::default_for_test()
    };
    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        node_stakes: vec![DEFAULT_NODE_STAKE],
        validator_configs: vec![validator_config],
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
    let client = cluster
        .get_validator_client(cluster.entry_point_info.pubkey())
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
    let leader_pubkey = *cluster.entry_point_info.pubkey();
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
    let node_stakes = vec![50 * DEFAULT_NODE_STAKE, 51 * DEFAULT_NODE_STAKE];
    let validator_keys: Vec<_> = [
        "4qhhXNTbKD1a5vxDDLZcHKj7ELNeiivtUBxn3wUK1F5VRsQVP89VUhfXqSfgiFB14GfuBgtrQ96n9NvWQADVkcCg",
        "3kHBzVwie5vTEaY6nFCPeFT8qDpoXzn7dCEioGRNBTnUDpvwnG85w8Wq63gVWpVTP8k2a8cgcWRjSXyUkEygpXWS",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .take(node_stakes.len())
    .collect();

    // Do not restart the validator which is the cluster entrypoint because its gossip port
    // might be changed after restart resulting in the two nodes not being able to
    // to form a cluster. The heavier validator is the second node.
    let node_to_restart = validator_keys[1].0.pubkey();

    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + node_stakes.iter().sum::<u64>(),
        node_stakes: node_stakes.clone(),
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default_for_test(),
            node_stakes.len(),
        ),
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
    // Let the nodes run for a while. Wait for validators to vote on slot `S`
    // so that the vote on `S-1` is definitely in gossip and optimistic confirmation is
    // detected on slot `S-1` for sure, then stop the heavier of the two
    // validators
    let client = cluster.get_validator_client(&node_to_restart).unwrap();
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

    let exited_validator_info = cluster.exit_node(&node_to_restart);

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
        // 2) Won't reset to this earlier ancestor because reset can only happen on same voted fork if
        // it's for the last vote slot or later
        remove_tower(&exited_validator_info.info.ledger_path, &node_to_restart);
        blockstore.set_dead_slot(prev_voted_slot).unwrap();
    }

    {
        // Buffer stderr to detect optimistic slot violation log
        let buf = std::env::var("OPTIMISTIC_CONF_TEST_DUMP_LOG")
            .err()
            .map(|_| BufferRedirect::stderr().unwrap());
        cluster.restart_node(
            &node_to_restart,
            exited_validator_info,
            SocketAddrSpace::Unspecified,
        );

        // Wait for a root > prev_voted_slot to be set. Because the root is on a
        // different fork than `prev_voted_slot`, then optimistic confirmation is
        // violated
        let client = cluster.get_validator_client(&node_to_restart).unwrap();
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
            print!("{output}");
            assert!(success);
        } else {
            panic!("dumped log and disabled testing");
        }
    }

    // Make sure validator still makes progress
    cluster_tests::check_for_new_roots(
        16,
        &[cluster.get_contact_info(&node_to_restart).unwrap().clone()],
        &cluster.connection_cache,
        "test_optimistic_confirmation_violation",
    );
}

#[test]
#[serial]
fn test_validator_saves_tower() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);

    let validator_config = ValidatorConfig {
        require_tower: true,
        ..ValidatorConfig::default_for_test()
    };
    let validator_identity_keypair = Arc::new(Keypair::new());
    let validator_id = validator_identity_keypair.pubkey();
    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        node_stakes: vec![DEFAULT_NODE_STAKE],
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
    loop {
        if let Ok(slot) = validator_client.get_slot_with_commitment(CommitmentConfig::processed()) {
            trace!("current slot: {}", slot);
            if slot > 2 {
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
    assert!(tower1.last_voted_slot().is_some());

    // Restart the validator and wait for a new root
    cluster.restart_node(&validator_id, validator_info, SocketAddrSpace::Unspecified);
    let validator_client = cluster.get_validator_client(&validator_id).unwrap();

    // Wait for the first new root
    let last_replayed_root = loop {
        #[allow(deprecated)]
        // This test depends on knowing the immediate root, without any delay from the commitment
        // service, so the deprecated CommitmentConfig::root() is retained
        if let Ok(root) = validator_client.get_slot_with_commitment(CommitmentConfig::root()) {
            trace!("current root: {}", root);
            if root > 0 {
                break root;
            }
        }
        sleep(Duration::from_millis(50));
    };

    // Stop validator, and check saved tower
    let validator_info = cluster.exit_node(&validator_id);
    let tower2 = Tower::restore(&file_tower_storage, &validator_id).unwrap();
    trace!("tower2: {:?}", tower2);
    assert_eq!(tower2.root(), last_replayed_root);

    // Rollback saved tower to `tower1` to simulate a validator starting from a newer snapshot
    // without having to wait for that snapshot to be generated in this test
    tower1
        .save(&file_tower_storage, &validator_identity_keypair)
        .unwrap();

    cluster.restart_node(&validator_id, validator_info, SocketAddrSpace::Unspecified);
    let validator_client = cluster.get_validator_client(&validator_id).unwrap();

    // Wait for a new root, demonstrating the validator was able to make progress from the older `tower1`
    let new_root = loop {
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
                break root;
            }
        }
        sleep(Duration::from_millis(50));
    };

    // Check the new root is reflected in the saved tower state
    let mut validator_info = cluster.exit_node(&validator_id);
    let tower3 = Tower::restore(&file_tower_storage, &validator_id).unwrap();
    trace!("tower3: {:?}", tower3);
    let tower3_root = tower3.root();
    assert!(tower3_root >= new_root);

    // Remove the tower file entirely and allow the validator to start without a tower.  It will
    // rebuild tower from its vote account contents
    remove_tower(&ledger_path, &validator_id);
    validator_info.config.require_tower = false;

    cluster.restart_node(&validator_id, validator_info, SocketAddrSpace::Unspecified);
    let validator_client = cluster.get_validator_client(&validator_id).unwrap();

    // Wait for another new root
    let new_root = loop {
        #[allow(deprecated)]
        // This test depends on knowing the immediate root, without any delay from the commitment
        // service, so the deprecated CommitmentConfig::root() is retained
        if let Ok(root) = validator_client.get_slot_with_commitment(CommitmentConfig::root()) {
            trace!("current root: {}, last tower root: {}", root, tower3_root);
            if root > tower3_root {
                break root;
            }
        }
        sleep(Duration::from_millis(50));
    };

    cluster.close_preserve_ledgers();

    let tower4 = Tower::restore(&file_tower_storage, &validator_id).unwrap();
    trace!("tower4: {:?}", tower4);
    assert!(tower4.root() >= new_root);
}

fn root_in_tower(tower_path: &Path, node_pubkey: &Pubkey) -> Option<Slot> {
    restore_tower(tower_path, node_pubkey).map(|tower| tower.root())
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
        ClusterMode::MasterOnly => vec![DEFAULT_NODE_STAKE],
        ClusterMode::MasterSlave => vec![DEFAULT_NODE_STAKE * 100, DEFAULT_NODE_STAKE],
    };

    let validator_keys = [
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
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + DEFAULT_NODE_STAKE * 100,
        node_stakes: node_stakes.clone(),
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default_for_test(),
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
        purge_slots_with_count(&blockstore, purged_slot_before_restart, 100);
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

fn restart_whole_cluster_after_hard_fork(
    cluster: &Arc<Mutex<LocalCluster>>,
    validator_a_pubkey: Pubkey,
    validator_b_pubkey: Pubkey,
    mut validator_a_info: ClusterValidatorInfo,
    validator_b_info: ClusterValidatorInfo,
) {
    // restart validator A first
    let cluster_for_a = cluster.clone();
    let val_a_ledger_path = validator_a_info.info.ledger_path.clone();

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
}

#[test]
fn test_hard_fork_invalidates_tower() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);

    // First set up the cluster with 2 nodes
    let slots_per_epoch = 2048;
    let node_stakes = vec![60 * DEFAULT_NODE_STAKE, 40 * DEFAULT_NODE_STAKE];

    let validator_keys = [
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
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + node_stakes.iter().sum::<u64>(),
        node_stakes: node_stakes.clone(),
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default_for_test(),
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
    // hard fork earlier than root is very unrealistic in the wild, but it's handy for
    // persistent tower's lockout behavior...
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

    // Clear ledger of all slots post hard fork
    {
        let blockstore_a = open_blockstore(&validator_a_info.info.ledger_path);
        let blockstore_b = open_blockstore(&validator_b_info.info.ledger_path);
        purge_slots_with_count(&blockstore_a, hard_fork_slot + 1, 100);
        purge_slots_with_count(&blockstore_b, hard_fork_slot + 1, 100);
    }

    restart_whole_cluster_after_hard_fork(
        &cluster,
        validator_a_pubkey,
        validator_b_pubkey,
        validator_a_info,
        validator_b_info,
    );

    // new slots should be rooted after hard-fork cluster relaunch
    cluster
        .lock()
        .unwrap()
        .check_for_new_roots(16, "hard fork", SocketAddrSpace::Unspecified);
}

#[test]
#[serial]
fn test_run_test_load_program_accounts_root() {
    run_test_load_program_accounts(CommitmentConfig::finalized());
}

fn create_simple_snapshot_config(ledger_path: &Path) -> SnapshotConfig {
    SnapshotConfig {
        full_snapshot_archives_dir: ledger_path.to_path_buf(),
        bank_snapshots_dir: ledger_path.join("snapshot"),
        ..SnapshotConfig::default()
    }
}

fn create_snapshot_to_hard_fork(
    blockstore: &Blockstore,
    snapshot_slot: Slot,
    hard_forks: Vec<Slot>,
) {
    let process_options = ProcessOptions {
        halt_at_slot: Some(snapshot_slot),
        new_hard_forks: Some(hard_forks),
        run_verification: false,
        ..ProcessOptions::default()
    };
    let ledger_path = blockstore.ledger_path();
    let genesis_config = open_genesis_config(ledger_path, u64::max_value());
    let snapshot_config = create_simple_snapshot_config(ledger_path);
    let (bank_forks, ..) = bank_forks_utils::load(
        &genesis_config,
        blockstore,
        vec![
            create_accounts_run_and_snapshot_dirs(ledger_path.join("accounts"))
                .unwrap()
                .0,
        ],
        None,
        Some(&snapshot_config),
        process_options,
        None,
        None,
        None,
        None,
        Arc::default(),
    )
    .unwrap();
    let bank = bank_forks.read().unwrap().get(snapshot_slot).unwrap();
    let full_snapshot_archive_info = snapshot_bank_utils::bank_to_full_snapshot_archive(
        ledger_path,
        &bank,
        Some(snapshot_config.snapshot_version),
        ledger_path,
        ledger_path,
        snapshot_config.archive_format,
        NonZeroUsize::new(1).unwrap(),
        NonZeroUsize::new(1).unwrap(),
    )
    .unwrap();
    info!(
        "Successfully created snapshot for slot {}, hash {}: {}",
        bank.slot(),
        bank.hash(),
        full_snapshot_archive_info.path().display(),
    );
}

#[test]
#[serial]
fn test_hard_fork_with_gap_in_roots() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);

    // First set up the cluster with 2 nodes
    let slots_per_epoch = 2048;
    let node_stakes = vec![60, 40];

    let validator_keys = [
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

    let validator_config = ValidatorConfig {
        snapshot_config: LocalCluster::create_dummy_load_only_snapshot_config(),
        ..ValidatorConfig::default()
    };
    let mut config = ClusterConfig {
        cluster_lamports: 100_000,
        node_stakes: node_stakes.clone(),
        validator_configs: make_identical_validator_configs(&validator_config, node_stakes.len()),
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
    let val_b_ledger_path = cluster.lock().unwrap().ledger_path(&validator_b_pubkey);

    let min_last_vote = 45;
    let min_root = 10;
    loop {
        sleep(Duration::from_millis(100));

        if let Some((last_vote, _)) = last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey) {
            if last_vote >= min_last_vote
                && root_in_tower(&val_a_ledger_path, &validator_a_pubkey) > Some(min_root)
            {
                break;
            }
        }
    }

    // stop all nodes of the cluster
    let mut validator_a_info = cluster.lock().unwrap().exit_node(&validator_a_pubkey);
    let mut validator_b_info = cluster.lock().unwrap().exit_node(&validator_b_pubkey);

    // hard fork slot is effectively a (possibly skipping) new root.
    // assert that the precondition of validator a to test gap between
    // blockstore and hard fork...
    let hard_fork_slot = min_last_vote - 5;
    assert!(hard_fork_slot > root_in_tower(&val_a_ledger_path, &validator_a_pubkey).unwrap());

    let hard_fork_slots = Some(vec![hard_fork_slot]);
    let mut hard_forks = HardForks::default();
    hard_forks.register(hard_fork_slot);

    let expected_shred_version = solana_sdk::shred_version::compute_shred_version(
        &cluster.lock().unwrap().genesis_config.hash(),
        Some(&hard_forks),
    );

    // create hard-forked snapshot only for validator a, emulating the manual cluster restart
    // procedure with `solana-ledger-tool create-snapshot`
    let genesis_slot = 0;
    {
        let blockstore_a = Blockstore::open(&val_a_ledger_path).unwrap();
        create_snapshot_to_hard_fork(&blockstore_a, hard_fork_slot, vec![hard_fork_slot]);

        // Intentionally make solana-validator unbootable by replaying blocks from the genesis to
        // ensure the hard-forked snapshot is used always.  Otherwise, we couldn't create a gap
        // in the ledger roots column family reliably.
        // There was a bug which caused the hard-forked snapshot at an unrooted slot to forget
        // to root some slots (thus, creating a gap in roots, which shouldn't happen).
        purge_slots_with_count(&blockstore_a, genesis_slot, 1);

        let next_slot = genesis_slot + 1;
        let mut meta = blockstore_a.meta(next_slot).unwrap().unwrap();
        meta.unset_parent();
        blockstore_a.put_meta(next_slot, &meta).unwrap();
    }

    // strictly speaking, new_hard_forks isn't needed for validator a.
    // but when snapshot loading isn't working, you might see:
    //   shred version mismatch: expected NNNN found: MMMM
    //validator_a_info.config.new_hard_forks = hard_fork_slots.clone();

    // effectively pass the --hard-fork parameter to validator b
    validator_b_info.config.new_hard_forks = hard_fork_slots;

    validator_a_info.config.wait_for_supermajority = Some(hard_fork_slot);
    validator_a_info.config.expected_shred_version = Some(expected_shred_version);

    validator_b_info.config.wait_for_supermajority = Some(hard_fork_slot);
    validator_b_info.config.expected_shred_version = Some(expected_shred_version);

    restart_whole_cluster_after_hard_fork(
        &cluster,
        validator_a_pubkey,
        validator_b_pubkey,
        validator_a_info,
        validator_b_info,
    );
    // new slots should be rooted after hard-fork cluster relaunch
    cluster
        .lock()
        .unwrap()
        .check_for_new_roots(16, "hard fork", SocketAddrSpace::Unspecified);

    // drop everything to open blockstores below
    drop(cluster);

    let (common_last_vote, common_root) = {
        let (last_vote_a, _) = last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey).unwrap();
        let (last_vote_b, _) = last_vote_in_tower(&val_b_ledger_path, &validator_b_pubkey).unwrap();
        let root_a = root_in_tower(&val_a_ledger_path, &validator_a_pubkey).unwrap();
        let root_b = root_in_tower(&val_b_ledger_path, &validator_b_pubkey).unwrap();
        (last_vote_a.min(last_vote_b), root_a.min(root_b))
    };

    let blockstore_a = Blockstore::open(&val_a_ledger_path).unwrap();
    let blockstore_b = Blockstore::open(&val_b_ledger_path).unwrap();

    // collect all slot/root parents
    let mut slots_a = AncestorIterator::new(common_last_vote, &blockstore_a).collect::<Vec<_>>();
    let mut roots_a = blockstore_a
        .reversed_rooted_slot_iterator(common_root)
        .unwrap()
        .collect::<Vec<_>>();
    // artifically restore the forcibly purged genesis only for the validator A just for the sake of
    // the final assertions.
    slots_a.push(genesis_slot);
    roots_a.push(genesis_slot);

    let slots_b = AncestorIterator::new(common_last_vote, &blockstore_b).collect::<Vec<_>>();
    let roots_b = blockstore_b
        .reversed_rooted_slot_iterator(common_root)
        .unwrap()
        .collect::<Vec<_>>();

    // compare them all!
    assert_eq!((&slots_a, &roots_a), (&slots_b, &roots_b));
    assert_eq!(&slots_a[slots_a.len() - roots_a.len()..].to_vec(), &roots_a);
    assert_eq!(&slots_b[slots_b.len() - roots_b.len()..].to_vec(), &roots_b);
}

#[test]
#[serial]
fn test_restart_tower_rollback() {
    // Test node crashing and failing to save its tower before restart
    // Cluster continues to make progress, this node is able to rejoin with
    // outdated tower post restart.
    solana_logger::setup_with_default(RUST_LOG_FILTER);

    // First set up the cluster with 2 nodes
    let slots_per_epoch = 2048;
    let node_stakes = vec![DEFAULT_NODE_STAKE * 100, DEFAULT_NODE_STAKE];

    let validator_strings = [
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
        "2saHBBoTkLMmttmPQP8KfBkcCw45S5cwtV3wTdGCscRC8uxdgvHxpHiWXKx4LvJjNJtnNcbSv5NdheokFFqnNDt8",
    ];

    let validator_keys = validator_strings
        .iter()
        .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
        .take(node_stakes.len())
        .collect::<Vec<_>>();

    let b_pubkey = validator_keys[1].0.pubkey();

    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + DEFAULT_NODE_STAKE * 100,
        node_stakes: node_stakes.clone(),
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default_for_test(),
            node_stakes.len(),
        ),
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let val_b_ledger_path = cluster.ledger_path(&b_pubkey);

    let mut earlier_tower: Tower;
    loop {
        sleep(Duration::from_millis(1000));

        // Grab the current saved tower
        earlier_tower = restore_tower(&val_b_ledger_path, &b_pubkey).unwrap();
        if earlier_tower.last_voted_slot().unwrap_or(0) > 1 {
            break;
        }
    }

    let mut exited_validator_info: ClusterValidatorInfo;
    let last_voted_slot: Slot;
    loop {
        sleep(Duration::from_millis(1000));

        // Wait for second, lesser staked validator to make a root past the earlier_tower's
        // latest vote slot, then exit that validator
        let tower = restore_tower(&val_b_ledger_path, &b_pubkey).unwrap();
        if tower.root()
            > earlier_tower
                .last_voted_slot()
                .expect("Earlier tower must have at least one vote")
        {
            exited_validator_info = cluster.exit_node(&b_pubkey);
            last_voted_slot = tower.last_voted_slot().unwrap();
            break;
        }
    }

    // Now rewrite the tower with the *earlier_tower*. We disable voting until we reach
    // a slot we did not previously vote for in order to avoid duplicate vote slashing
    // issues.
    save_tower(
        &val_b_ledger_path,
        &earlier_tower,
        &exited_validator_info.info.keypair,
    );
    exited_validator_info.config.wait_to_vote_slot = Some(last_voted_slot + 10);

    cluster.restart_node(
        &b_pubkey,
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
    let partitions: [usize; 2] = [1, 1];
    let (leader_schedule, validator_keys) = create_custom_leader_schedule_with_random_keys(&[
        num_slots_per_validator,
        num_slots_per_validator,
    ]);

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
            .get_validator_client(cluster.entry_point_info.pubkey())
            .unwrap();
        update_client_sender.send(update_client).unwrap();
        let scan_client = cluster
            .get_validator_client(cluster.entry_point_info.pubkey())
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
        additional_accounts,
    );
}

#[test]
#[serial]
#[ignore]
fn test_rpc_block_subscribe() {
    let total_stake = 100 * DEFAULT_NODE_STAKE;
    let leader_stake = total_stake;
    let node_stakes = vec![leader_stake];
    let mut validator_config = ValidatorConfig::default_for_test();
    validator_config.enable_default_rpc_block_subscribe();

    let validator_keys = [
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .take(node_stakes.len())
    .collect::<Vec<_>>();

    let mut config = ClusterConfig {
        cluster_lamports: total_stake,
        node_stakes,
        validator_configs: vec![validator_config],
        validator_keys: Some(validator_keys),
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
    let (mut block_subscribe_client, receiver) = PubsubClient::block_subscribe(
        &format!(
            "ws://{}",
            &cluster.entry_point_info.rpc_pubsub().unwrap().to_string()
        ),
        RpcBlockSubscribeFilter::All,
        Some(RpcBlockSubscribeConfig {
            commitment: Some(CommitmentConfig::confirmed()),
            encoding: None,
            transaction_details: None,
            show_rewards: None,
            max_supported_transaction_version: None,
        }),
    )
    .unwrap();

    let mut received_block = false;
    let max_wait = 10_000;
    let start = Instant::now();
    while !received_block {
        assert!(
            start.elapsed() <= Duration::from_millis(max_wait),
            "Went too long {max_wait} ms without receiving a confirmed block",
        );
        let responses: Vec<_> = receiver.try_iter().collect();
        // Wait for a response
        if !responses.is_empty() {
            for response in responses {
                assert!(response.value.err.is_none());
                assert!(response.value.block.is_some());
                if response.value.slot > 1 {
                    received_block = true;
                }
            }
        }
        sleep(Duration::from_millis(100));
    }

    // If we don't drop the cluster, the blocking web socket service
    // won't return, and the `block_subscribe_client` won't shut down
    drop(cluster);
    block_subscribe_client.shutdown().unwrap();
}

#[test]
#[serial]
#[allow(unused_attributes)]
fn test_oc_bad_signatures() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);

    let total_stake = 100 * DEFAULT_NODE_STAKE;
    let leader_stake = (total_stake as f64 * VOTE_THRESHOLD_SIZE) as u64;
    let our_node_stake = total_stake - leader_stake;
    let node_stakes = vec![leader_stake, our_node_stake];
    let mut validator_config = ValidatorConfig {
        require_tower: true,
        ..ValidatorConfig::default_for_test()
    };
    validator_config.enable_default_rpc_block_subscribe();
    let validator_keys = [
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
        "2saHBBoTkLMmttmPQP8KfBkcCw45S5cwtV3wTdGCscRC8uxdgvHxpHiWXKx4LvJjNJtnNcbSv5NdheokFFqnNDt8",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .take(node_stakes.len())
    .collect::<Vec<_>>();

    let our_id = validator_keys.last().unwrap().0.pubkey();
    let mut config = ClusterConfig {
        cluster_lamports: total_stake,
        node_stakes,
        validator_configs: make_identical_validator_configs(&validator_config, 2),
        validator_keys: Some(validator_keys),
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    // 2) Kill our node and start up a thread to simulate votes to control our voting behavior
    let our_info = cluster.exit_node(&our_id);
    let node_keypair = our_info.info.keypair;
    let vote_keypair = our_info.info.voting_keypair;
    info!(
        "our node id: {}, vote id: {}",
        node_keypair.pubkey(),
        vote_keypair.pubkey()
    );

    // 3) Start up a spy to listen for and push votes to leader TPU
    let (rpc, tpu) = LegacyContactInfo::try_from(&cluster.entry_point_info)
        .map(|node| {
            cluster_tests::get_client_facing_addr(cluster.connection_cache.protocol(), node)
        })
        .unwrap();
    let client = ThinClient::new(rpc, tpu, cluster.connection_cache.clone());
    let cluster_funding_keypair = cluster.funding_keypair.insecure_clone();
    let voter_thread_sleep_ms: usize = 100;
    let num_votes_simulated = Arc::new(AtomicUsize::new(0));
    let gossip_voter = cluster_tests::start_gossip_voter(
        &cluster.entry_point_info.gossip().unwrap(),
        &node_keypair,
        |(_label, leader_vote_tx)| {
            let vote = vote_parser::parse_vote_transaction(&leader_vote_tx)
                .map(|(_, vote, ..)| vote)
                .unwrap();
            // Filter out empty votes
            if !vote.is_empty() {
                Some((vote, leader_vote_tx))
            } else {
                None
            }
        },
        {
            let node_keypair = node_keypair.insecure_clone();
            let vote_keypair = vote_keypair.insecure_clone();
            let num_votes_simulated = num_votes_simulated.clone();
            move |vote_slot, leader_vote_tx, parsed_vote, _cluster_info| {
                info!("received vote for {}", vote_slot);
                let vote_hash = parsed_vote.hash();
                info!(
                    "Simulating vote from our node on slot {}, hash {}",
                    vote_slot, vote_hash
                );

                // Add all recent vote slots on this fork to allow cluster to pass
                // vote threshold checks in replay. Note this will instantly force a
                // root by this validator.
                let vote_slots: Vec<Slot> = vec![vote_slot];

                let bad_authorized_signer_keypair = Keypair::new();
                let mut vote_tx = vote_transaction::new_vote_transaction(
                    vote_slots,
                    vote_hash,
                    leader_vote_tx.message.recent_blockhash,
                    &node_keypair,
                    &vote_keypair,
                    // Make a bad signer
                    &bad_authorized_signer_keypair,
                    None,
                );
                client
                    .retry_transfer(&cluster_funding_keypair, &mut vote_tx, 5)
                    .unwrap();

                num_votes_simulated.fetch_add(1, Ordering::Relaxed);
            }
        },
        voter_thread_sleep_ms as u64,
    );

    let (mut block_subscribe_client, receiver) = PubsubClient::block_subscribe(
        &format!(
            "ws://{}",
            &cluster.entry_point_info.rpc_pubsub().unwrap().to_string()
        ),
        RpcBlockSubscribeFilter::All,
        Some(RpcBlockSubscribeConfig {
            commitment: Some(CommitmentConfig::confirmed()),
            encoding: None,
            transaction_details: None,
            show_rewards: None,
            max_supported_transaction_version: None,
        }),
    )
    .unwrap();

    const MAX_VOTES_TO_SIMULATE: usize = 10;
    // Make sure test doesn't take too long
    assert!(voter_thread_sleep_ms * MAX_VOTES_TO_SIMULATE <= 1000);
    loop {
        let responses: Vec<_> = receiver.try_iter().collect();
        // Nothing should get optimistically confirmed or rooted
        assert!(responses.is_empty());
        // Wait for the voter thread to attempt sufficient number of votes to give
        // a chance for the violation to occur
        if num_votes_simulated.load(Ordering::Relaxed) > MAX_VOTES_TO_SIMULATE {
            break;
        }
        sleep(Duration::from_millis(100));
    }

    // Clean up voter thread
    gossip_voter.close();

    // If we don't drop the cluster, the blocking web socket service
    // won't return, and the `block_subscribe_client` won't shut down
    drop(cluster);
    block_subscribe_client.shutdown().unwrap();
}

#[test]
#[serial]
#[ignore]
fn test_votes_land_in_fork_during_long_partition() {
    let total_stake = 3 * DEFAULT_NODE_STAKE;
    // Make `lighter_stake` insufficient for switching threshold
    let lighter_stake = (SWITCH_FORK_THRESHOLD * total_stake as f64) as u64;
    let heavier_stake = lighter_stake + 1;
    let failures_stake = total_stake - lighter_stake - heavier_stake;

    // Give lighter stake 30 consecutive slots before
    // the heavier stake gets a single slot
    let partitions: &[(usize, usize)] =
        &[(heavier_stake as usize, 1), (lighter_stake as usize, 30)];

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
                start.elapsed() <= Duration::from_millis(max_wait),
                "Went too long {max_wait} ms without a root",
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
        &[(failures_stake as usize, 0)],
        partitions,
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
                    let result = client.async_transfer(
                        1,
                        &starting_keypairs_[i],
                        &target_keypairs_[i].pubkey(),
                        blockhash,
                    );
                    if result.is_err() {
                        debug!("Failed in transfer for starting keypair: {:?}", result);
                    }
                }
                for i in 0..starting_keypairs_.len() {
                    let result = client.async_transfer(
                        1,
                        &target_keypairs_[i],
                        &starting_keypairs_[i].pubkey(),
                        blockhash,
                    );
                    if result.is_err() {
                        debug!("Failed in transfer for starting keypair: {:?}", result);
                    }
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
    let node_stakes = vec![51 * DEFAULT_NODE_STAKE, 50 * DEFAULT_NODE_STAKE];
    let validator_keys: Vec<_> = [
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
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + node_stakes.iter().sum::<u64>(),
        node_stakes: node_stakes.clone(),
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default_for_test(),
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
        .find(|x| x != cluster.entry_point_info.pubkey())
        .unwrap();
    let client = cluster
        .get_validator_client(cluster.entry_point_info.pubkey())
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

// A bit convoluted test case; but this roughly follows this test theoretical scenario:
// Validator A, B, C have 31, 36, 33 % of stake respectively. Leader schedule is split, first half
// of the test B is always leader, second half C is. Additionally we have a non voting validator D with 0
// stake to propagate gossip info.
//
// Step 1: Kill C, only A, B and D should be running
//
//  S0 -> S1 -> S2 -> S3 (A & B vote, optimistically confirmed)
//
// Step 2:
// Kill A and B once we verify that they have voted on S3 or beyond. Copy B's ledger to C but only
// up to slot S2
// Have `C` generate some blocks like:
//
// S0 -> S1 -> S2 -> S4
//
// Step 3: Then restart `A` which had 31% of the stake, and remove S3 from its ledger, so
// that it only sees `C`'s fork at S2. From `A`'s perspective it sees:
//
// S0 -> S1 -> S2
//             |
//             -> S4 -> S5 (C's vote for S4)
//
// The fork choice rule weights look like:
//
// S0 -> S1 -> S2 (ABC)
//             |
//             -> S4 (C) -> S5
//
// Step 5:
// Without the persisted tower:
//    `A` would choose to vote on the fork with `S4 -> S5`.
//
// With the persisted tower:
//    `A` should not be able to generate a switching proof.
//
fn do_test_optimistic_confirmation_violation_with_or_without_tower(with_tower: bool) {
    solana_logger::setup_with("debug");

    // First set up the cluster with 4 nodes
    let slots_per_epoch = 2048;
    let node_stakes = vec![
        31 * DEFAULT_NODE_STAKE,
        36 * DEFAULT_NODE_STAKE,
        33 * DEFAULT_NODE_STAKE,
        0,
    ];

    let base_slot: Slot = 26; // S2
    let next_slot_on_a: Slot = 27; // S3
    let truncated_slots: Slot = 100; // just enough to purge all following slots after the S2 and S3

    // Each pubkeys are prefixed with A, B, C and D.
    // D is needed to:
    // 1) Propagate A's votes for S2 to validator C after A shuts down so that
    // C can avoid NoPropagatedConfirmation errors and continue to generate blocks
    // 2) Provide gossip discovery for `A` when it restarts because `A` will restart
    // at a different gossip port than the entrypoint saved in C's gossip table
    let validator_keys = [
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
    // to the other forks, and may violate switching proofs on restart.
    let mut default_config = ValidatorConfig::default_for_test();
    // Split leader schedule 50-50 between validators B and C, don't give validator A any slots because
    // it's going to be deleting its ledger, so may create versions of slots it's already created, but
    // on a different fork.
    let validator_to_slots = vec![
        // Ensure validator b is leader for slots <= `next_slot_on_a`
        (validator_b_pubkey, next_slot_on_a as usize + 1),
        (validator_c_pubkey, next_slot_on_a as usize + 1),
    ];

    let leader_schedule = create_custom_leader_schedule(validator_to_slots.into_iter());
    for slot in 0..=next_slot_on_a {
        assert_eq!(leader_schedule[slot], validator_b_pubkey);
    }

    default_config.fixed_leader_schedule = Some(FixedSchedule {
        leader_schedule: Arc::new(leader_schedule),
    });
    let mut validator_configs =
        make_identical_validator_configs(&default_config, node_stakes.len());

    // Disable voting on validators C, and D
    validator_configs[2].voting_disabled = true;
    validator_configs[3].voting_disabled = true;

    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + node_stakes.iter().sum::<u64>(),
        node_stakes,
        validator_configs,
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

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

    // Immediately kill validator C. No need to kill validator A because
    // 1) It has no slots in the leader schedule, so no way to make forks
    // 2) We need it to vote
    info!("Exiting validator C");
    let mut validator_c_info = cluster.exit_node(&validator_c_pubkey);

    // Step 1:
    // Let validator A, B, (D) run. Wait for both `A` and `B` to have voted on `next_slot_on_a` or
    // one of its descendants
    info!(
        "Waiting on both validators A and B to vote on fork at slot {}",
        next_slot_on_a
    );
    let now = Instant::now();
    let mut last_b_vote = 0;
    let mut last_a_vote = 0;
    loop {
        let elapsed = now.elapsed();
        assert!(
            elapsed <= Duration::from_secs(30),
            "One of the validators failed to vote on a slot >= {} in {} secs,
            last validator A vote: {},
            last validator B vote: {}",
            next_slot_on_a,
            elapsed.as_secs(),
            last_a_vote,
            last_b_vote,
        );
        sleep(Duration::from_millis(100));

        if let Some((last_vote, _)) = last_vote_in_tower(&val_b_ledger_path, &validator_b_pubkey) {
            last_b_vote = last_vote;
            if last_vote < next_slot_on_a {
                continue;
            }
        }

        if let Some((last_vote, _)) = last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey) {
            last_a_vote = last_vote;
            if last_vote >= next_slot_on_a {
                break;
            }
        }
    }

    // kill A and B
    let _validator_b_info = cluster.exit_node(&validator_b_pubkey);
    let validator_a_info = cluster.exit_node(&validator_a_pubkey);

    // Step 2:
    // Truncate ledger, copy over B's ledger to C
    info!("Create validator C's ledger");
    {
        // first copy from validator B's ledger
        std::fs::remove_dir_all(&validator_c_info.info.ledger_path).unwrap();
        let mut opt = fs_extra::dir::CopyOptions::new();
        opt.copy_inside = true;
        fs_extra::dir::copy(&val_b_ledger_path, &val_c_ledger_path, &opt).unwrap();
        // Remove B's tower in C's new copied ledger
        remove_tower(&val_c_ledger_path, &validator_b_pubkey);

        let blockstore = open_blockstore(&val_c_ledger_path);
        purge_slots_with_count(&blockstore, base_slot + 1, truncated_slots);
    }
    info!("Create validator A's ledger");
    {
        // Find latest vote in B, and wait for it to reach blockstore
        let b_last_vote =
            wait_for_last_vote_in_tower_to_land_in_ledger(&val_b_ledger_path, &validator_b_pubkey)
                .unwrap();

        // Now we copy these blocks to A
        let b_blockstore = open_blockstore(&val_b_ledger_path);
        let a_blockstore = open_blockstore(&val_a_ledger_path);
        copy_blocks(b_last_vote, &b_blockstore, &a_blockstore);

        // Purge uneccessary slots
        purge_slots_with_count(&a_blockstore, next_slot_on_a + 1, truncated_slots);
    }

    // This should be guaranteed because we waited for validator `A` to vote on a slot > `next_slot_on_a`
    // before killing it earlier.
    info!("Checking A's tower for a vote on slot descended from slot `next_slot_on_a`");
    let last_vote_slot = last_vote_in_tower(&val_a_ledger_path, &validator_a_pubkey)
        .unwrap()
        .0;
    assert!(last_vote_slot >= next_slot_on_a);
    info!("Success, A voted on slot {}", last_vote_slot);

    {
        let blockstore = open_blockstore(&val_a_ledger_path);
        if !with_tower {
            info!("Removing tower!");
            remove_tower(&val_a_ledger_path, &validator_a_pubkey);

            // Remove next_slot_on_a from ledger to force validator A to select
            // votes_on_c_fork. Otherwise, in the test case without a tower,
            // the validator A will immediately vote for 27 on restart, because it
            // hasn't gotten the heavier fork from validator C yet.
            // Then it will be stuck on 27 unable to switch because C doesn't
            // have enough stake to generate a switching proof
            purge_slots_with_count(&blockstore, next_slot_on_a, truncated_slots);
        } else {
            info!("Not removing tower!");
        }
    }

    // Step 3:
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

    // Step 4:
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
            let blockstore = open_blockstore(&val_a_ledger_path);
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

#[test]
#[serial]
#[ignore]
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
    let total_stake = 100 * DEFAULT_NODE_STAKE;
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
    let alive_stake_3 = 2 * DEFAULT_NODE_STAKE;
    assert!(alive_stake_1 < alive_stake_2);
    assert!(alive_stake_1 + alive_stake_3 > alive_stake_2);

    let partitions: &[(usize, usize)] = &[
        (alive_stake_1 as usize, 8),
        (alive_stake_2 as usize, 8),
        (alive_stake_3 as usize, 0),
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
            )
            .unwrap();
            let heaviest_fork_latest_vote = wait_for_last_vote_in_tower_to_land_in_ledger(
                &heaviest_ledger_path,
                &context.heaviest_validator_key,
            )
            .unwrap();

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
        &[(failures_stake as usize - 1, 16)],
        partitions,
        Some(ticks_per_slot),
        PartitionContext::default(),
        on_partition_start,
        on_before_partition_resolved,
        on_partition_resolved,
    );
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
    let partitions: [usize; 4] = [
        11 * DEFAULT_NODE_STAKE as usize,
        10 * DEFAULT_NODE_STAKE as usize,
        10 * DEFAULT_NODE_STAKE as usize,
        10 * DEFAULT_NODE_STAKE as usize,
    ];
    let (leader_schedule, validator_keys) = create_custom_leader_schedule_with_random_keys(&[
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
        vec![],
    )
}

#[test]
#[serial]
fn test_kill_partition_switch_threshold_no_progress() {
    let max_switch_threshold_failure_pct = 1.0 - 2.0 * SWITCH_FORK_THRESHOLD;
    let total_stake = 10_000 * DEFAULT_NODE_STAKE;
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
        &[(failures_stake as usize, 16)],
        &[(alive_stake_1 as usize, 8), (alive_stake_2 as usize, 8)],
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
    let total_stake = 10_000 * DEFAULT_NODE_STAKE;

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
        &[(failures_stake as usize, 16)],
        &[(alive_stake_1 as usize, 8), (alive_stake_2 as usize, 8)],
        None,
        (),
        on_partition_start,
        on_before_partition_resolved,
        on_partition_resolved,
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
    let bad_leader_stake = 10_000_000 * DEFAULT_NODE_STAKE;
    // Ensure that the good_node_stake is always on the critical path, and the partition node
    // should never be on the critical path. This way, none of the bad shreds sent to the partition
    // node corrupt the good node.
    let good_node_stake = 500 * DEFAULT_NODE_STAKE;
    let our_node_stake = 10_000_000 * DEFAULT_NODE_STAKE;
    let partition_node_stake = DEFAULT_NODE_STAKE;

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
            partition: ClusterPartition::Stake(partition_node_stake),
            duplicate_slot_sender: None,
        }),
        node_stakes,
        None,
        None,
    );

    // This is why it's important our node was last in `node_stakes`
    let our_id = validator_keys.last().unwrap().pubkey();

    // 2) Kill our node and start up a thread to simulate votes to control our voting behavior
    let our_info = cluster.exit_node(&our_id);
    let node_keypair = our_info.info.keypair;
    let vote_keypair = our_info.info.voting_keypair;
    let bad_leader_id = *cluster.entry_point_info.pubkey();
    let bad_leader_ledger_path = cluster.validators[&bad_leader_id].info.ledger_path.clone();
    info!("our node id: {}", node_keypair.pubkey());

    // 3) Start up a gossip instance to listen for and push votes
    let voter_thread_sleep_ms = 100;
    let gossip_voter = cluster_tests::start_gossip_voter(
        &cluster.entry_point_info.gossip().unwrap(),
        &node_keypair,
        move |(label, leader_vote_tx)| {
            // Filter out votes not from the bad leader
            if label.pubkey() == bad_leader_id {
                let vote = vote_parser::parse_vote_transaction(&leader_vote_tx)
                    .map(|(_, vote, ..)| vote)
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
        },
        {
            let node_keypair = node_keypair.insecure_clone();
            let vote_keypair = vote_keypair.insecure_clone();
            let mut max_vote_slot = 0;
            let mut gossip_vote_index = 0;
            move |latest_vote_slot, leader_vote_tx, parsed_vote, cluster_info| {
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
                    let mut vote_slots: Vec<(Slot, u32)> =
                        AncestorIterator::new_inclusive(latest_vote_slot, &leader_blockstore)
                            .take(MAX_LOCKOUT_HISTORY)
                            .zip(1..)
                            .collect();
                    vote_slots.reverse();
                    let mut vote = VoteStateUpdate::from(vote_slots);
                    let root =
                        AncestorIterator::new_inclusive(latest_vote_slot, &leader_blockstore)
                            .nth(MAX_LOCKOUT_HISTORY);
                    vote.root = root;
                    vote.hash = vote_hash;
                    let vote_tx = vote_transaction::new_compact_vote_state_update_transaction(
                        vote,
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
        },
        voter_thread_sleep_ms as u64,
    );

    // 4) Check that the cluster is making progress
    cluster.check_for_new_roots(
        16,
        "test_duplicate_shreds_broadcast_leader",
        SocketAddrSpace::Unspecified,
    );

    // Clean up threads
    gossip_voter.close();
}

#[test]
#[serial]
#[ignore]
fn test_switch_threshold_uses_gossip_votes() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let total_stake = 100 * DEFAULT_NODE_STAKE;

    // Minimum stake needed to generate a switching proof
    let minimum_switch_stake = (SWITCH_FORK_THRESHOLD * total_stake as f64) as u64;

    // Make the heavier stake insufficient for switching so tha the lighter validator
    // cannot switch without seeing a vote from the dead/failure_stake validator.
    let heavier_stake = minimum_switch_stake;
    let lighter_stake = heavier_stake - 1;
    let failures_stake = total_stake - heavier_stake - lighter_stake;

    let partitions: &[(usize, usize)] = &[(heavier_stake as usize, 8), (lighter_stake as usize, 8)];

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
                .gossip()
                .unwrap(),
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
        &[(failures_stake as usize, 0)],
        partitions,
        Some(ticks_per_slot),
        PartitionContext::default(),
        on_partition_start,
        on_before_partition_resolved,
        on_partition_resolved,
    );
}

#[test]
#[serial]
fn test_listener_startup() {
    let mut config = ClusterConfig {
        node_stakes: vec![DEFAULT_NODE_STAKE],
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        num_listeners: 3,
        validator_configs: make_identical_validator_configs(
            &ValidatorConfig::default_for_test(),
            1,
        ),
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);
    let cluster_nodes = discover_cluster(
        &cluster.entry_point_info.gossip().unwrap(),
        4,
        SocketAddrSpace::Unspecified,
    )
    .unwrap();
    assert_eq!(cluster_nodes.len(), 4);
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

        if let Some(new_latest_slot) = new_latest_slots.first() {
            latest_slot = *new_latest_slot;
            info!("Checking latest_slot {}", latest_slot);
            // Wait for the slot to be fully received by the validator
            loop {
                info!("Waiting for slot {} to be full", latest_slot);
                if blockstore.is_full(latest_slot) {
                    break;
                } else {
                    sleep(Duration::from_millis(50));
                    blockstore = open_blockstore(ledger_path);
                }
            }
            // Wait for the slot to be replayed
            loop {
                info!("Waiting for slot {} to be replayed", latest_slot);
                if blockstore.get_bank_hash(latest_slot).is_some() {
                    return (
                        latest_slot,
                        AncestorIterator::new(latest_slot, &blockstore).collect(),
                    );
                } else {
                    sleep(Duration::from_millis(50));
                    blockstore = open_blockstore(ledger_path);
                }
            }
        }
        sleep(Duration::from_millis(50));
    }
}

#[test]
#[serial]
fn test_cluster_partition_1_1() {
    let empty = |_: &mut LocalCluster, _: &mut ()| {};
    let on_partition_resolved = |cluster: &mut LocalCluster, _: &mut ()| {
        cluster.check_for_new_roots(16, "PARTITION_TEST", SocketAddrSpace::Unspecified);
    };
    run_cluster_partition(
        &[1, 1],
        None,
        (),
        empty,
        empty,
        on_partition_resolved,
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
        &[1, 1, 1],
        None,
        (),
        empty,
        empty,
        on_partition_resolved,
        None,
        vec![],
    )
}

// Cluster needs a supermajority to remain, so the minimum size for this test is 4
#[test]
#[serial]
fn test_leader_failure_4() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_leader_failure_4");
    let num_nodes = 4;
    let validator_config = ValidatorConfig::default_for_test();
    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        node_stakes: vec![DEFAULT_NODE_STAKE; 4],
        validator_configs: make_identical_validator_configs(&validator_config, num_nodes),
        ..ClusterConfig::default()
    };
    let local = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    cluster_tests::kill_entry_and_spend_and_verify_rest(
        &local.entry_point_info,
        &local
            .validators
            .get(local.entry_point_info.pubkey())
            .unwrap()
            .config
            .validator_exit,
        &local.funding_keypair,
        &local.connection_cache,
        num_nodes,
        config.ticks_per_slot * config.poh_config.target_tick_duration.as_millis() as u64,
        SocketAddrSpace::Unspecified,
    );
}

#[test]
#[serial]
fn test_ledger_cleanup_service() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    error!("test_ledger_cleanup_service");
    let num_nodes = 3;
    let validator_config = ValidatorConfig {
        max_ledger_shreds: Some(100),
        ..ValidatorConfig::default_for_test()
    };
    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        poh_config: PohConfig::new_sleep(Duration::from_millis(50)),
        node_stakes: vec![DEFAULT_NODE_STAKE; num_nodes],
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
        &cluster.connection_cache,
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
        assert!(slots <= 103, "got {slots}");
    }
}

// This test verifies that even if votes from a validator end up taking too long to land, and thus
// some of the referenced slots are slots are no longer present in the slot hashes sysvar,
// consensus can still be attained.
//
// Validator A (60%)
// Validator B (40%)
//                                  / --- 10 --- [..] --- 16 (B is voting, due to network issues is initally not able to see the other fork at all)
//                                 /
// 1 - 2 - 3 - 4 - 5 - 6 - 7 - 8 - 9 (A votes 1 - 9 votes are landing normally. B does the same however votes are not landing)
//                                 \
//                                  \--[..]-- 73  (majority fork)
// A is voting on the majority fork and B wants to switch to this fork however in this majority fork
// the earlier votes for B (1 - 9) never landed so when B eventually goes to vote on 73, slots in
// its local vote state are no longer present in slot hashes.
//
// 1. Wait for B's tower to see local vote state was updated to new fork
// 2. Wait X blocks, check B's vote state on chain has been properly updated
//
// NOTE: it is not reliable for B to organically have 1 to reach 2^16 lockout, so we simulate the 6
// consecutive votes on the minor fork by manually incrementing the confirmation levels for the
// common ancestor votes in tower.
// To allow this test to run in a reasonable time we change the
// slot_hash expiry to 64 slots.

#[test]
fn test_slot_hash_expiry() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    solana_sdk::slot_hashes::set_entries_for_tests_only(64);

    let slots_per_epoch = 2048;
    let node_stakes = vec![60 * DEFAULT_NODE_STAKE, 40 * DEFAULT_NODE_STAKE];
    let validator_keys = [
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
        "2saHBBoTkLMmttmPQP8KfBkcCw45S5cwtV3wTdGCscRC8uxdgvHxpHiWXKx4LvJjNJtnNcbSv5NdheokFFqnNDt8",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .collect::<Vec<_>>();
    let node_vote_keys = [
        "3NDQ3ud86RTVg8hTy2dDWnS4P8NfjhZ2gDgQAJbr3heaKaUVS1FW3sTLKA1GmDrY9aySzsa4QxpDkbLv47yHxzr3",
        "46ZHpHE6PEvXYPu3hf9iQqjBk2ZNDaJ9ejqKWHEjxaQjpAGasKaWKbKHbP3646oZhfgDRzx95DH9PCBKKsoCVngk",
    ]
    .iter()
    .map(|s| Arc::new(Keypair::from_base58_string(s)))
    .collect::<Vec<_>>();
    let vs = validator_keys
        .iter()
        .map(|(kp, _)| kp.pubkey())
        .collect::<Vec<_>>();
    let (a_pubkey, b_pubkey) = (vs[0], vs[1]);

    // We want B to not vote (we are trying to simulate its votes not landing until it gets to the
    // minority fork)
    let mut validator_configs =
        make_identical_validator_configs(&ValidatorConfig::default_for_test(), node_stakes.len());
    validator_configs[1].voting_disabled = true;

    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + node_stakes.iter().sum::<u64>(),
        node_stakes,
        validator_configs,
        validator_keys: Some(validator_keys),
        node_vote_keys: Some(node_vote_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let mut common_ancestor_slot = 8;

    let a_ledger_path = cluster.ledger_path(&a_pubkey);
    let b_ledger_path = cluster.ledger_path(&b_pubkey);

    // Immediately kill B (we just needed it for the initial stake distribution)
    info!("Killing B");
    let mut b_info = cluster.exit_node(&b_pubkey);

    // Let A run for a while until we get to the common ancestor
    info!("Letting A run until common_ancestor_slot");
    loop {
        if let Some((last_vote, _)) = last_vote_in_tower(&a_ledger_path, &a_pubkey) {
            if last_vote >= common_ancestor_slot {
                break;
            }
        }
        sleep(Duration::from_millis(100));
    }

    // Keep A running, but setup B so that it thinks it has voted up until common ancestor (but
    // doesn't know anything past that)
    {
        info!("Copying A's ledger to B");
        std::fs::remove_dir_all(&b_info.info.ledger_path).unwrap();
        let mut opt = fs_extra::dir::CopyOptions::new();
        opt.copy_inside = true;
        fs_extra::dir::copy(&a_ledger_path, &b_ledger_path, &opt).unwrap();

        // remove A's tower in B's new copied ledger
        info!("Removing A's tower in B's ledger dir");
        remove_tower(&b_ledger_path, &a_pubkey);

        // load A's tower and save it as B's tower
        info!("Loading A's tower");
        if let Some(mut a_tower) = restore_tower(&a_ledger_path, &a_pubkey) {
            a_tower.node_pubkey = b_pubkey;
            // Update common_ancestor_slot because A is still running
            if let Some(s) = a_tower.last_voted_slot() {
                common_ancestor_slot = s;
                info!("New common_ancestor_slot {}", common_ancestor_slot);
            } else {
                panic!("A's tower has no votes");
            }
            info!("Increase lockout by 6 confirmation levels and save as B's tower");
            a_tower.increase_lockout(6);
            save_tower(&b_ledger_path, &a_tower, &b_info.info.keypair);
            info!("B's new tower: {:?}", a_tower.tower_slots());
        } else {
            panic!("A's tower is missing");
        }

        // Get rid of any slots past common_ancestor_slot
        info!("Removing extra slots from B's blockstore");
        let blockstore = open_blockstore(&b_ledger_path);
        purge_slots_with_count(&blockstore, common_ancestor_slot + 1, 100);
    }

    info!(
        "Run A on majority fork until it reaches slot hash expiry {}",
        solana_sdk::slot_hashes::get_entries()
    );
    let mut last_vote_on_a;
    // Keep A running for a while longer so the majority fork has some decent size
    loop {
        last_vote_on_a =
            wait_for_last_vote_in_tower_to_land_in_ledger(&a_ledger_path, &a_pubkey).unwrap();
        if last_vote_on_a
            >= common_ancestor_slot + 2 * (solana_sdk::slot_hashes::get_entries() as u64)
        {
            let blockstore = open_blockstore(&a_ledger_path);
            info!(
                "A majority fork: {:?}",
                AncestorIterator::new(last_vote_on_a, &blockstore).collect::<Vec<Slot>>()
            );
            break;
        }
        sleep(Duration::from_millis(100));
    }

    // Kill A and restart B with voting. B should now fork off
    info!("Killing A");
    let a_info = cluster.exit_node(&a_pubkey);

    info!("Restarting B");
    b_info.config.voting_disabled = false;
    cluster.restart_node(&b_pubkey, b_info, SocketAddrSpace::Unspecified);

    // B will fork off and accumulate enough lockout
    info!("Allowing B to fork");
    loop {
        let blockstore = open_blockstore(&b_ledger_path);
        let last_vote =
            wait_for_last_vote_in_tower_to_land_in_ledger(&b_ledger_path, &b_pubkey).unwrap();
        let mut ancestors = AncestorIterator::new(last_vote, &blockstore);
        if let Some(index) = ancestors.position(|x| x == common_ancestor_slot) {
            if index > 7 {
                info!(
                    "B has forked for enough lockout: {:?}",
                    AncestorIterator::new(last_vote, &blockstore).collect::<Vec<Slot>>()
                );
                break;
            }
        }
        sleep(Duration::from_millis(1000));
    }

    info!("Kill B");
    b_info = cluster.exit_node(&b_pubkey);

    info!("Resolve the partition");
    {
        // Here we let B know about the missing blocks that A had produced on its partition
        let a_blockstore = open_blockstore(&a_ledger_path);
        let b_blockstore = open_blockstore(&b_ledger_path);
        copy_blocks(last_vote_on_a, &a_blockstore, &b_blockstore);
    }

    // Now restart A and B and see if B is able to eventually switch onto the majority fork
    info!("Restarting A & B");
    cluster.restart_node(&a_pubkey, a_info, SocketAddrSpace::Unspecified);
    cluster.restart_node(&b_pubkey, b_info, SocketAddrSpace::Unspecified);

    info!("Waiting for B to switch to majority fork and make a root");
    cluster_tests::check_for_new_roots(
        16,
        &[cluster.get_contact_info(&a_pubkey).unwrap().clone()],
        &cluster.connection_cache,
        "test_slot_hashes_expiry",
    );
}

// This test simulates a case where a leader sends a duplicate block with different ancestory. One
// version builds off of the rooted path, however the other version builds off a pruned branch. The
// validators that receive the pruned version will need to repair in order to continue, which
// requires an ancestor hashes repair.
//
// We setup 3 validators:
// - majority, will produce the rooted path
// - minority, will produce the pruned path
// - our_node, will be fed the pruned version of the duplicate block and need to repair
//
// Additionally we setup 3 observer nodes to propagate votes and participate in the ancestor hashes
// sample.
//
// Fork structure:
//
// 0 - 1 - ... - 10 (fork slot) - 30 - ... - 61 (rooted path) - ...
//                |
//                |- 11 - ... - 29 (pruned path) - 81'
//
//
// Steps:
// 1) Different leader schedule, minority thinks it produces 0-29 and majority rest, majority
//    thinks minority produces all blocks. This is to avoid majority accidentally producing blocks
//    before it shuts down.
// 2) Start cluster, kill our_node.
// 3) Kill majority cluster after it votes for any slot > fork slot (guarantees that the fork slot is
//    reached as minority cannot pass threshold otherwise).
// 4) Let minority produce forks on pruned forks until out of leader slots then kill.
// 5) Truncate majority ledger past fork slot so it starts building off of fork slot.
// 6) Restart majority and wait untill it starts producing blocks on main fork and roots something
//    past the fork slot.
// 7) Construct our ledger by copying majority ledger and copying blocks from minority for the pruned path.
// 8) In our node's ledger, change the parent of the latest slot in majority fork to be the latest
//    slot in the minority fork (simulates duplicate built off of pruned block)
// 9) Start our node which will pruned the minority fork on ledger replay and verify that we can make roots.
//
#[test]
#[serial]
fn test_duplicate_with_pruned_ancestor() {
    solana_logger::setup_with("info,solana_metrics=off");
    solana_core::repair::duplicate_repair_status::set_ancestor_hash_repair_sample_size_for_tests_only(3);

    let majority_leader_stake = 10_000_000 * DEFAULT_NODE_STAKE;
    let minority_leader_stake = 2_000_000 * DEFAULT_NODE_STAKE;
    let our_node = DEFAULT_NODE_STAKE;
    let observer_stake = DEFAULT_NODE_STAKE;

    let slots_per_epoch = 2048;
    let fork_slot: u64 = 10;
    let fork_length: u64 = 20;
    let majority_fork_buffer = 5;

    let mut node_stakes = vec![majority_leader_stake, minority_leader_stake, our_node];
    // We need enough observers to reach `ANCESTOR_HASH_REPAIR_SAMPLE_SIZE`
    node_stakes.append(&mut vec![observer_stake; 3]);

    let num_nodes = node_stakes.len();

    let validator_keys = [
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
        "2saHBBoTkLMmttmPQP8KfBkcCw45S5cwtV3wTdGCscRC8uxdgvHxpHiWXKx4LvJjNJtnNcbSv5NdheokFFqnNDt8",
        "4mx9yoFBeYasDKBGDWCTWGJdWuJCKbgqmuP8bN9umybCh5Jzngw7KQxe99Rf5uzfyzgba1i65rJW4Wqk7Ab5S8ye",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .chain(std::iter::repeat_with(|| (Arc::new(Keypair::new()), true)))
    .take(node_stakes.len())
    .collect::<Vec<_>>();
    let validators = validator_keys
        .iter()
        .map(|(kp, _)| kp.pubkey())
        .collect::<Vec<_>>();
    let (majority_pubkey, minority_pubkey, our_node_pubkey) =
        (validators[0], validators[1], validators[2]);

    let mut default_config = ValidatorConfig::default_for_test();
    // Minority fork is leader long enough to create pruned fork
    let validator_to_slots = vec![
        (minority_pubkey, (fork_slot + fork_length) as usize),
        (majority_pubkey, slots_per_epoch as usize),
    ];
    let leader_schedule = create_custom_leader_schedule(validator_to_slots.into_iter());
    default_config.fixed_leader_schedule = Some(FixedSchedule {
        leader_schedule: Arc::new(leader_schedule),
    });

    let mut validator_configs = make_identical_validator_configs(&default_config, num_nodes);
    // Don't let majority produce anything past the fork by tricking its leader schedule
    validator_configs[0].fixed_leader_schedule = Some(FixedSchedule {
        leader_schedule: Arc::new(create_custom_leader_schedule(
            [(minority_pubkey, slots_per_epoch as usize)].into_iter(),
        )),
    });

    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + node_stakes.iter().sum::<u64>(),
        node_stakes,
        validator_configs,
        validator_keys: Some(validator_keys),
        slots_per_epoch,
        stakers_slot_offset: slots_per_epoch,
        skip_warmup_slots: true,
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut config, SocketAddrSpace::Unspecified);

    let majority_ledger_path = cluster.ledger_path(&majority_pubkey);
    let minority_ledger_path = cluster.ledger_path(&minority_pubkey);
    let our_node_ledger_path = cluster.ledger_path(&our_node_pubkey);

    info!(
        "majority {} ledger path {:?}",
        majority_pubkey, majority_ledger_path
    );
    info!(
        "minority {} ledger path {:?}",
        minority_pubkey, minority_ledger_path
    );
    info!(
        "our_node {} ledger path {:?}",
        our_node_pubkey, our_node_ledger_path
    );

    info!("Killing our node");
    let our_node_info = cluster.exit_node(&our_node_pubkey);

    info!("Waiting on majority validator to vote on at least {fork_slot}");
    let now = Instant::now();
    let mut last_majority_vote = 0;
    loop {
        let elapsed = now.elapsed();
        assert!(
            elapsed <= Duration::from_secs(30),
            "Majority validator failed to vote on a slot >= {} in {} secs,
            majority validator last vote: {}",
            fork_slot,
            elapsed.as_secs(),
            last_majority_vote,
        );
        sleep(Duration::from_millis(100));

        if let Some((last_vote, _)) = last_vote_in_tower(&majority_ledger_path, &majority_pubkey) {
            last_majority_vote = last_vote;
            if last_vote >= fork_slot {
                break;
            }
        }
    }

    info!("Killing majority validator, waiting for minority fork to reach a depth of at least 15",);
    let mut majority_validator_info = cluster.exit_node(&majority_pubkey);

    let now = Instant::now();
    let mut last_minority_vote = 0;
    while last_minority_vote < fork_slot + 15 {
        let elapsed = now.elapsed();
        assert!(
            elapsed <= Duration::from_secs(30),
            "Minority validator failed to create a fork of depth >= {} in {} secs,
            last_minority_vote: {}",
            15,
            elapsed.as_secs(),
            last_minority_vote,
        );

        if let Some((last_vote, _)) = last_vote_in_tower(&minority_ledger_path, &minority_pubkey) {
            last_minority_vote = last_vote;
        }
    }

    info!(
        "Killing minority validator, fork created successfully: {:?}",
        last_minority_vote
    );
    let last_minority_vote =
        wait_for_last_vote_in_tower_to_land_in_ledger(&minority_ledger_path, &minority_pubkey)
            .unwrap();
    let minority_validator_info = cluster.exit_node(&minority_pubkey);

    info!("Truncating majority validator ledger to {fork_slot}");
    {
        remove_tower(&majority_ledger_path, &majority_pubkey);
        let blockstore = open_blockstore(&majority_ledger_path);
        purge_slots_with_count(&blockstore, fork_slot + 1, 100);
    }

    info!("Restarting majority validator");
    // Make sure we don't send duplicate votes
    majority_validator_info.config.wait_to_vote_slot = Some(fork_slot + fork_length);
    // Fix the leader schedule so we can produce blocks
    majority_validator_info.config.fixed_leader_schedule =
        minority_validator_info.config.fixed_leader_schedule.clone();
    cluster.restart_node(
        &majority_pubkey,
        majority_validator_info,
        SocketAddrSpace::Unspecified,
    );

    let mut last_majority_root = 0;
    let now = Instant::now();
    info!(
        "Waiting for majority validator to root something past {}",
        fork_slot + fork_length + majority_fork_buffer
    );
    while last_majority_root <= fork_slot + fork_length + majority_fork_buffer {
        let elapsed = now.elapsed();
        assert!(
            elapsed <= Duration::from_secs(60),
            "Majority validator failed to root something > {} in {} secs,
            last majority validator vote: {},",
            fork_slot + fork_length + majority_fork_buffer,
            elapsed.as_secs(),
            last_majority_vote,
        );
        sleep(Duration::from_millis(100));

        if let Some(last_root) = last_root_in_tower(&majority_ledger_path, &majority_pubkey) {
            last_majority_root = last_root;
        }
    }

    let last_majority_vote =
        wait_for_last_vote_in_tower_to_land_in_ledger(&majority_ledger_path, &majority_pubkey)
            .unwrap();
    info!(
        "Creating duplicate block built off of pruned branch for our node.
           Last majority vote {last_majority_vote}, Last minority vote {last_minority_vote}"
    );
    {
        {
            // Copy majority fork
            std::fs::remove_dir_all(&our_node_info.info.ledger_path).unwrap();
            let mut opt = fs_extra::dir::CopyOptions::new();
            opt.copy_inside = true;
            fs_extra::dir::copy(&majority_ledger_path, &our_node_ledger_path, &opt).unwrap();
            remove_tower(&our_node_ledger_path, &majority_pubkey);
        }

        // Copy minority fork. Rewind our root so that we can copy over the purged bank
        let minority_blockstore = open_blockstore(&minority_validator_info.info.ledger_path);
        let mut our_blockstore = open_blockstore(&our_node_info.info.ledger_path);
        our_blockstore.set_last_root(fork_slot - 1);
        copy_blocks(last_minority_vote, &minority_blockstore, &our_blockstore);

        // Change last block parent to chain off of (purged) minority fork
        info!("For our node, changing parent of {last_majority_vote} to {last_minority_vote}");
        our_blockstore.clear_unconfirmed_slot(last_majority_vote);
        let entries = create_ticks(
            64 * (std::cmp::max(1, last_majority_vote - last_minority_vote)),
            0,
            Hash::default(),
        );
        let shreds = entries_to_test_shreds(
            &entries,
            last_majority_vote,
            last_minority_vote,
            true,
            0,
            true, // merkle_variant
        );
        our_blockstore.insert_shreds(shreds, None, false).unwrap();

        // Update the root to set minority fork back as pruned
        our_blockstore.set_last_root(fork_slot + fork_length);
    }

    // Actual test, `our_node` will replay the minority fork, then the majority fork which will
    // prune the minority fork. Then finally the problematic block will be skipped (not replayed)
    // because its parent has been pruned from bank forks. Meanwhile the majority validator has
    // continued making blocks and voting, duplicate confirming everything. This will cause the
    // pruned fork to become popular triggering an ancestor hashes repair, eventually allowing our
    // node to dump & repair & continue making roots.
    info!("Restarting our node, verifying that our node is making roots past the duplicate block");

    cluster.restart_node(
        &our_node_pubkey,
        our_node_info,
        SocketAddrSpace::Unspecified,
    );

    cluster_tests::check_for_new_roots(
        16,
        &[cluster.get_contact_info(&our_node_pubkey).unwrap().clone()],
        &cluster.connection_cache,
        "test_duplicate_with_pruned_ancestor",
    );
}

/// Test fastboot to ensure a node can boot from local state and still produce correct snapshots
///
/// 1. Start node 1 and wait for it to take snapshots
/// 2. Start node 2 with the snapshots from (1)
/// 3. Wait for node 2 to take a bank snapshot
/// 4. Restart node 2 with the local state from (3)
/// 5. Wait for node 2 to take new snapshots
/// 6. Start node 3 with the snapshots from (5)
/// 7. Wait for node 3 to take new snapshots
/// 8. Ensure the snapshots from (7) match node's 1 and 2
#[test]
#[serial]
fn test_boot_from_local_state() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    const FULL_SNAPSHOT_INTERVAL: Slot = 100;
    const INCREMENTAL_SNAPSHOT_INTERVAL: Slot = 10;

    let validator1_config = SnapshotValidatorConfig::new(
        FULL_SNAPSHOT_INTERVAL,
        INCREMENTAL_SNAPSHOT_INTERVAL,
        INCREMENTAL_SNAPSHOT_INTERVAL,
        2,
    );
    let validator2_config = SnapshotValidatorConfig::new(
        FULL_SNAPSHOT_INTERVAL,
        INCREMENTAL_SNAPSHOT_INTERVAL,
        INCREMENTAL_SNAPSHOT_INTERVAL,
        4,
    );
    let validator3_config = SnapshotValidatorConfig::new(
        FULL_SNAPSHOT_INTERVAL,
        INCREMENTAL_SNAPSHOT_INTERVAL,
        INCREMENTAL_SNAPSHOT_INTERVAL,
        3,
    );

    let mut cluster_config = ClusterConfig {
        node_stakes: vec![100 * DEFAULT_NODE_STAKE],
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        validator_configs: make_identical_validator_configs(&validator1_config.validator_config, 1),
        ..ClusterConfig::default()
    };
    let mut cluster = LocalCluster::new(&mut cluster_config, SocketAddrSpace::Unspecified);

    // in order to boot from local state, need to first have snapshot archives
    info!("Waiting for validator1 to create snapshots...");
    let (incremental_snapshot_archive, full_snapshot_archive) =
        LocalCluster::wait_for_next_incremental_snapshot(
            &cluster,
            &validator1_config.full_snapshot_archives_dir,
            &validator1_config.incremental_snapshot_archives_dir,
            Some(Duration::from_secs(5 * 60)),
        );
    debug!("snapshot archives:\n\tfull: {full_snapshot_archive:?}\n\tincr: {incremental_snapshot_archive:?}");
    info!("Waiting for validator1 to create snapshots... DONE");

    info!("Copying snapshots to validator2...");
    std::fs::copy(
        full_snapshot_archive.path(),
        validator2_config
            .full_snapshot_archives_dir
            .path()
            .join(full_snapshot_archive.path().file_name().unwrap()),
    )
    .unwrap();
    std::fs::copy(
        incremental_snapshot_archive.path(),
        validator2_config
            .incremental_snapshot_archives_dir
            .path()
            .join(incremental_snapshot_archive.path().file_name().unwrap()),
    )
    .unwrap();
    info!("Copying snapshots to validator2... DONE");

    info!("Starting validator2...");
    let validator2_identity = Arc::new(Keypair::new());
    cluster.add_validator(
        &validator2_config.validator_config,
        DEFAULT_NODE_STAKE,
        validator2_identity.clone(),
        None,
        SocketAddrSpace::Unspecified,
    );
    info!("Starting validator2... DONE");

    // wait for a new bank snapshot to fastboot from that is newer than its snapshot archives
    info!("Waiting for validator2 to create a new bank snapshot...");
    let timer = Instant::now();
    let bank_snapshot = loop {
        if let Some(full_snapshot_slot) = snapshot_utils::get_highest_full_snapshot_archive_slot(
            &validator2_config.full_snapshot_archives_dir,
        ) {
            if let Some(incremental_snapshot_slot) =
                snapshot_utils::get_highest_incremental_snapshot_archive_slot(
                    &validator2_config.incremental_snapshot_archives_dir,
                    full_snapshot_slot,
                )
            {
                if let Some(bank_snapshot) = snapshot_utils::get_highest_bank_snapshot_post(
                    &validator2_config.bank_snapshots_dir,
                ) {
                    if bank_snapshot.slot > incremental_snapshot_slot {
                        break bank_snapshot;
                    }
                }
            }
        }
        assert!(
            timer.elapsed() < Duration::from_secs(30),
            "It should not take longer than 30 seconds to create a new bank snapshot"
        );
        std::thread::yield_now();
    };
    debug!("bank snapshot: {bank_snapshot:?}");
    info!("Waiting for validator2 to create a new bank snapshot... DONE");

    // restart WITH fastboot
    info!("Restarting validator2 from local state...");
    let mut validator2_info = cluster.exit_node(&validator2_identity.pubkey());
    validator2_info.config.use_snapshot_archives_at_startup = UseSnapshotArchivesAtStartup::Never;
    cluster.restart_node(
        &validator2_identity.pubkey(),
        validator2_info,
        SocketAddrSpace::Unspecified,
    );
    info!("Restarting validator2 from local state... DONE");

    info!("Waiting for validator2 to create snapshots...");
    let (incremental_snapshot_archive, full_snapshot_archive) =
        LocalCluster::wait_for_next_incremental_snapshot(
            &cluster,
            &validator2_config.full_snapshot_archives_dir,
            &validator2_config.incremental_snapshot_archives_dir,
            Some(Duration::from_secs(5 * 60)),
        );
    debug!("snapshot archives:\n\tfull: {full_snapshot_archive:?}\n\tincr: {incremental_snapshot_archive:?}");
    info!("Waiting for validator2 to create snapshots... DONE");

    info!("Copying snapshots to validator3...");
    std::fs::copy(
        full_snapshot_archive.path(),
        validator3_config
            .full_snapshot_archives_dir
            .path()
            .join(full_snapshot_archive.path().file_name().unwrap()),
    )
    .unwrap();
    std::fs::copy(
        incremental_snapshot_archive.path(),
        validator3_config
            .incremental_snapshot_archives_dir
            .path()
            .join(incremental_snapshot_archive.path().file_name().unwrap()),
    )
    .unwrap();
    info!("Copying snapshots to validator3... DONE");

    info!("Starting validator3...");
    let validator3_identity = Arc::new(Keypair::new());
    cluster.add_validator(
        &validator3_config.validator_config,
        DEFAULT_NODE_STAKE,
        validator3_identity,
        None,
        SocketAddrSpace::Unspecified,
    );
    info!("Starting validator3... DONE");

    // wait for a new snapshot to ensure the validator is making roots
    info!("Waiting for validator3 to create snapshots...");
    let (incremental_snapshot_archive, full_snapshot_archive) =
        LocalCluster::wait_for_next_incremental_snapshot(
            &cluster,
            &validator3_config.full_snapshot_archives_dir,
            &validator3_config.incremental_snapshot_archives_dir,
            Some(Duration::from_secs(5 * 60)),
        );
    debug!("snapshot archives:\n\tfull: {full_snapshot_archive:?}\n\tincr: {incremental_snapshot_archive:?}");
    info!("Waiting for validator3 to create snapshots... DONE");

    // ensure that all validators have the correct state by comparing snapshots
    // - wait for the other validators to have high enough snapshots
    // - ensure validator3's snapshot hashes match the other validators' snapshot hashes
    //
    // NOTE: There's a chance validator's 1 or 2 have crossed the next full snapshot past what
    // validator 3 has.  If that happens, validator's 1 or 2 may have purged the snapshots needed
    // to compare with validator 3, and thus assert.  If that happens, the full snapshot interval
    // may need to be adjusted larger.
    for (i, other_validator_config) in [(1, &validator1_config), (2, &validator2_config)] {
        info!("Checking if validator{i} has the same snapshots as validator3...");
        let timer = Instant::now();
        loop {
            if let Some(other_full_snapshot_slot) =
                snapshot_utils::get_highest_full_snapshot_archive_slot(
                    &other_validator_config.full_snapshot_archives_dir,
                )
            {
                let other_incremental_snapshot_slot =
                    snapshot_utils::get_highest_incremental_snapshot_archive_slot(
                        &other_validator_config.incremental_snapshot_archives_dir,
                        other_full_snapshot_slot,
                    );
                if other_full_snapshot_slot >= full_snapshot_archive.slot()
                    && other_incremental_snapshot_slot >= Some(incremental_snapshot_archive.slot())
                {
                    break;
                }
            }
            assert!(
                timer.elapsed() < Duration::from_secs(60),
                "It should not take longer than 60 seconds to take snapshots"
            );
            std::thread::yield_now();
        }
        let other_full_snapshot_archives = snapshot_utils::get_full_snapshot_archives(
            &other_validator_config.full_snapshot_archives_dir,
        );
        debug!("validator{i} full snapshot archives: {other_full_snapshot_archives:?}");
        assert!(other_full_snapshot_archives
            .iter()
            .any(
                |other_full_snapshot_archive| other_full_snapshot_archive.slot()
                    == full_snapshot_archive.slot()
                    && other_full_snapshot_archive.hash() == full_snapshot_archive.hash()
            ));

        let other_incremental_snapshot_archives = snapshot_utils::get_incremental_snapshot_archives(
            &other_validator_config.incremental_snapshot_archives_dir,
        );
        debug!(
            "validator{i} incremental snapshot archives: {other_incremental_snapshot_archives:?}"
        );
        assert!(other_incremental_snapshot_archives.iter().any(
            |other_incremental_snapshot_archive| other_incremental_snapshot_archive.base_slot()
                == incremental_snapshot_archive.base_slot()
                && other_incremental_snapshot_archive.slot() == incremental_snapshot_archive.slot()
                && other_incremental_snapshot_archive.hash() == incremental_snapshot_archive.hash()
        ));
        info!("Checking if validator{i} has the same snapshots as validator3... DONE");
    }
}

// We want to simulate the following:
//   /--- 1 --- 3 (duplicate block)
// 0
//   \--- 2
//
// 1. > DUPLICATE_THRESHOLD of the nodes vote on some version of the the duplicate block 3,
// but don't immediately duplicate confirm so they remove 3 from fork choice and reset PoH back to 1.
// 2. All the votes on 3 don't land because there are no further blocks building off 3.
// 3. Some < SWITCHING_THRESHOLD of nodes vote on 2, making it the heaviest fork because no votes on 3 landed
// 4. Nodes then see duplicate confirmation on 3.
// 5. Unless somebody builds off of 3 to include the duplicate confirmed votes, 2 will still be the heaviest.
// However, because 2 has < SWITCHING_THRESHOLD of the votes, people who voted on 3 can't switch, leading to a
// stall
#[test]
#[serial]
#[allow(unused_attributes)]
fn test_duplicate_shreds_switch_failure() {
    fn wait_for_duplicate_fork_frozen(ledger_path: &Path, dup_slot: Slot) -> Hash {
        // Ensure all the slots <= dup_slot are also full so we know we can replay up to dup_slot
        // on restart
        info!(
            "Waiting to receive and replay entire duplicate fork with tip {}",
            dup_slot
        );
        loop {
            let duplicate_fork_validator_blockstore = open_blockstore(ledger_path);
            if let Some(frozen_hash) = duplicate_fork_validator_blockstore.get_bank_hash(dup_slot) {
                return frozen_hash;
            }
            sleep(Duration::from_millis(1000));
        }
    }

    fn clear_ledger_and_tower(ledger_path: &Path, pubkey: &Pubkey, start_slot: Slot) {
        remove_tower_if_exists(ledger_path, pubkey);
        let blockstore = open_blockstore(ledger_path);
        purge_slots_with_count(&blockstore, start_slot, 1000);
        {
            // Remove all duplicate proofs so that this dup_slot will vote on the `dup_slot`.
            while let Some((proof_slot, _)) = blockstore.get_first_duplicate_proof() {
                blockstore.remove_slot_duplicate_proof(proof_slot).unwrap();
            }
        }
    }

    fn restart_dup_validator(
        cluster: &mut LocalCluster,
        mut duplicate_fork_validator_info: ClusterValidatorInfo,
        pubkey: &Pubkey,
        dup_slot: Slot,
        dup_shred1: &Shred,
        dup_shred2: &Shred,
    ) {
        let disable_turbine = Arc::new(AtomicBool::new(true));
        duplicate_fork_validator_info.config.voting_disabled = false;
        duplicate_fork_validator_info.config.turbine_disabled = disable_turbine.clone();
        info!("Restarting node: {}", pubkey);
        cluster.restart_node(
            pubkey,
            duplicate_fork_validator_info,
            SocketAddrSpace::Unspecified,
        );
        let ledger_path = cluster.ledger_path(pubkey);

        // Lift the partition after `pubkey` votes on the `dup_slot`
        info!(
            "Waiting on duplicate fork to vote on duplicate slot: {}",
            dup_slot
        );
        loop {
            let last_vote = last_vote_in_tower(&ledger_path, pubkey);
            if let Some((latest_vote_slot, _hash)) = last_vote {
                info!("latest vote: {}", latest_vote_slot);
                if latest_vote_slot == dup_slot {
                    break;
                }
            }
            sleep(Duration::from_millis(1000));
        }
        disable_turbine.store(false, Ordering::Relaxed);

        // Send the validator the other version of the shred so they realize it's duplicate
        info!("Resending duplicate shreds to duplicate fork validator");
        cluster.send_shreds_to_validator(vec![dup_shred1, dup_shred2], pubkey);

        // Check the validator detected a duplicate proof
        info!("Waiting on duplicate fork validator to see duplicate shreds and make a proof",);
        loop {
            let duplicate_fork_validator_blockstore = open_blockstore(&ledger_path);
            if let Some(dup_proof) = duplicate_fork_validator_blockstore.get_first_duplicate_proof()
            {
                assert_eq!(dup_proof.0, dup_slot);
                break;
            }
            sleep(Duration::from_millis(1000));
        }
    }

    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let validator_keypairs = [
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
        "2saHBBoTkLMmttmPQP8KfBkcCw45S5cwtV3wTdGCscRC8uxdgvHxpHiWXKx4LvJjNJtnNcbSv5NdheokFFqnNDt8",
        "4mx9yoFBeYasDKBGDWCTWGJdWuJCKbgqmuP8bN9umybCh5Jzngw7KQxe99Rf5uzfyzgba1i65rJW4Wqk7Ab5S8ye",
        "2XFPyuzPuXMsPnkH98UNcQpfA7M4b2TUhRxcWEoWjy4M6ojQ7HGJSvotktEVbaq49Qxt16wUjdqvSJc6ecbFfZwj",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .collect::<Vec<_>>();

    let validators = validator_keypairs
        .iter()
        .map(|(kp, _)| kp.pubkey())
        .collect::<Vec<_>>();

    // Create 4 nodes:
    // 1) Two nodes that sum to > DUPLICATE_THRESHOLD but < 2/3+ supermajority. It's important both
    // of them individually have <= DUPLICATE_THRESHOLD to avoid duplicate confirming their own blocks
    // immediately upon voting
    // 2) One with <= SWITCHING_THRESHOLD so that validator from 1) can't switch to it
    // 3) One bad leader to make duplicate slots
    let total_stake = 100 * DEFAULT_NODE_STAKE;
    let target_switch_fork_stake = (total_stake as f64 * SWITCH_FORK_THRESHOLD) as u64;
    // duplicate_fork_node1_stake + duplicate_fork_node2_stake > DUPLICATE_THRESHOLD. Don't want
    // one node with > DUPLICATE_THRESHOLD, otherwise they will automatically duplicate confirm a
    // slot when they vote, which will prevent them from resetting to an earlier ancestor when they
    // later discover that slot as duplicate.
    let duplicate_fork_node1_stake = (total_stake as f64 * DUPLICATE_THRESHOLD) as u64;
    let duplicate_fork_node2_stake = 1;
    let duplicate_leader_stake = total_stake
        - target_switch_fork_stake
        - duplicate_fork_node1_stake
        - duplicate_fork_node2_stake;
    assert!(
        duplicate_fork_node1_stake + duplicate_fork_node2_stake
            > (total_stake as f64 * DUPLICATE_THRESHOLD) as u64
    );
    assert!(duplicate_fork_node1_stake <= (total_stake as f64 * DUPLICATE_THRESHOLD) as u64);
    assert!(duplicate_fork_node2_stake <= (total_stake as f64 * DUPLICATE_THRESHOLD) as u64);

    let node_stakes = vec![
        duplicate_leader_stake,
        target_switch_fork_stake,
        duplicate_fork_node1_stake,
        duplicate_fork_node2_stake,
    ];

    let (
        // Has to be first in order to be picked as the duplicate leader
        duplicate_leader_validator_pubkey,
        target_switch_fork_validator_pubkey,
        duplicate_fork_validator1_pubkey,
        duplicate_fork_validator2_pubkey,
    ) = (validators[0], validators[1], validators[2], validators[3]);

    info!(
        "duplicate_fork_validator1_pubkey: {},
        duplicate_fork_validator2_pubkey: {},
        target_switch_fork_validator_pubkey: {},
        duplicate_leader_validator_pubkey: {}",
        duplicate_fork_validator1_pubkey,
        duplicate_fork_validator2_pubkey,
        target_switch_fork_validator_pubkey,
        duplicate_leader_validator_pubkey
    );

    let validator_to_slots = vec![
        (duplicate_leader_validator_pubkey, 50),
        (target_switch_fork_validator_pubkey, 5),
        // The ideal sequence of events for the `duplicate_fork_validator1_pubkey` validator would go:
        // 1. Vote for duplicate block `D`
        // 2. See `D` is duplicate, remove from fork choice and reset to ancestor `A`, potentially generating a fork off that ancestor
        // 3. See `D` is duplicate confirmed, but because of the bug fixed by https://github.com/solana-labs/solana/pull/28172
        // where we disallow resetting to a slot which matches the last vote slot, we still don't build off `D`,
        // and continue building on `A`.
        //
        // The `target_switch_fork_validator_pubkey` fork is necessary in 2. to force the validator stall trying to switch
        // vote on that other fork and prevent the validator from making a freebie vote from `A` and allowing consensus to continue.

        // It's important we don't give the `duplicate_fork_validator1_pubkey` leader slots until a certain number
        // of slots have elapsed to ensure:
        // 1. We have ample time to ensure he doesn't have a chance to make a block until after 2 when they see the block is duplicate.
        // Otherwise, they'll build the block on top of the duplicate block, which will possibly include a vote for the duplicate block.
        // We want to avoid this because this will make fork choice pick the duplicate block.
        // 2. Ensure the `duplicate_fork_validator1_pubkey` sees the target switch fork before it can make another vote
        // on any forks he himself generates from A. Otherwise, he will make a freebie vote on his own fork from `A` and
        // consensus will continue on that fork.

        // Give the duplicate fork validator plenty of leader slots after the initial delay to prevent
        // 1. Switch fork from getting locked out for too long
        // 2. A lot of consecutive slots in which to build up lockout in tower and make new roots
        // to resolve the partition
        (duplicate_fork_validator1_pubkey, 500),
    ];

    let leader_schedule = create_custom_leader_schedule(validator_to_slots.into_iter());

    // 1) Set up the cluster
    let (duplicate_slot_sender, duplicate_slot_receiver) = unbounded();
    let validator_configs = validator_keypairs
        .into_iter()
        .map(|(validator_keypair, in_genesis)| {
            let pubkey = validator_keypair.pubkey();
            // Only allow the leader to vote so that no version gets duplicate confirmed.
            // This is to avoid the leader dumping his own block.
            let voting_disabled = { pubkey != duplicate_leader_validator_pubkey };
            ValidatorTestConfig {
                validator_keypair,
                validator_config: ValidatorConfig {
                    voting_disabled,
                    ..ValidatorConfig::default()
                },
                in_genesis,
            }
        })
        .collect();
    let (mut cluster, _validator_keypairs) = test_faulty_node(
        BroadcastStageType::BroadcastDuplicates(BroadcastDuplicatesConfig {
            partition: ClusterPartition::Pubkey(vec![
                // Don't include the other dup validator here, otherwise
                // this dup version will have enough to be duplicate confirmed and
                // will cause the dup leader to try and dump its own slot,
                // crashing before it can signal the duplicate slot via the
                // `duplicate_slot_receiver` below
                duplicate_fork_validator1_pubkey,
            ]),
            duplicate_slot_sender: Some(duplicate_slot_sender),
        }),
        node_stakes,
        Some(validator_configs),
        Some(FixedSchedule {
            leader_schedule: Arc::new(leader_schedule),
        }),
    );

    // Kill two validators that might duplicate confirm the duplicate block
    info!("Killing unnecessary validators");
    let duplicate_fork_validator2_ledger_path =
        cluster.ledger_path(&duplicate_fork_validator2_pubkey);
    let duplicate_fork_validator2_info = cluster.exit_node(&duplicate_fork_validator2_pubkey);
    let target_switch_fork_validator_ledger_path =
        cluster.ledger_path(&target_switch_fork_validator_pubkey);
    let mut target_switch_fork_validator_info =
        cluster.exit_node(&target_switch_fork_validator_pubkey);

    // 2) Wait for a duplicate slot to land on both validators and for the target switch
    // fork validator to get another version of the slot. Also ensure all versions of
    // the block are playable
    let dup_slot = duplicate_slot_receiver
        .recv_timeout(Duration::from_millis(30_000))
        .expect("Duplicate leader failed to make a duplicate slot in allotted time");

    // Make sure both validators received and replay the complete blocks
    let dup_frozen_hash = wait_for_duplicate_fork_frozen(
        &cluster.ledger_path(&duplicate_fork_validator1_pubkey),
        dup_slot,
    );
    let original_frozen_hash = wait_for_duplicate_fork_frozen(
        &cluster.ledger_path(&duplicate_leader_validator_pubkey),
        dup_slot,
    );
    assert_ne!(
        original_frozen_hash, dup_frozen_hash,
        "Duplicate leader and partition target got same hash: {original_frozen_hash}",
    );

    // 3) Force `duplicate_fork_validator1_pubkey` to see a duplicate proof
    info!("Waiting for duplicate proof for slot: {}", dup_slot);
    let duplicate_proof = {
        // Grab the other version of the slot from the `duplicate_leader_validator_pubkey`
        // which we confirmed to have a different version of the frozen hash in the loop
        // above
        let ledger_path = cluster.ledger_path(&duplicate_leader_validator_pubkey);
        let blockstore = open_blockstore(&ledger_path);
        let dup_shred = blockstore
            .get_data_shreds_for_slot(dup_slot, 0)
            .unwrap()
            .pop()
            .unwrap();
        info!(
            "Sending duplicate shred: {:?} to {:?}",
            dup_shred.signature(),
            duplicate_fork_validator1_pubkey
        );
        cluster.send_shreds_to_validator(vec![&dup_shred], &duplicate_fork_validator1_pubkey);
        wait_for_duplicate_proof(
            &cluster.ledger_path(&duplicate_fork_validator1_pubkey),
            dup_slot,
        )
        .unwrap_or_else(|| panic!("Duplicate proof for slot {} not found", dup_slot))
    };

    // 3) Kill all the validators
    info!("Killing remaining validators");
    let duplicate_fork_validator1_ledger_path =
        cluster.ledger_path(&duplicate_fork_validator1_pubkey);
    let duplicate_fork_validator1_info = cluster.exit_node(&duplicate_fork_validator1_pubkey);
    let duplicate_leader_ledger_path = cluster.ledger_path(&duplicate_leader_validator_pubkey);
    cluster.exit_node(&duplicate_leader_validator_pubkey);

    let dup_shred1 = Shred::new_from_serialized_shred(duplicate_proof.shred1.clone()).unwrap();
    let dup_shred2 = Shred::new_from_serialized_shred(duplicate_proof.shred2).unwrap();
    assert_eq!(dup_shred1.slot(), dup_shred2.slot());
    assert_eq!(dup_shred1.slot(), dup_slot);

    // Purge everything including the `dup_slot` from the `target_switch_fork_validator_pubkey`
    info!(
        "Purging towers and ledgers for: {:?}",
        duplicate_leader_validator_pubkey
    );
    Blockstore::destroy(&target_switch_fork_validator_ledger_path).unwrap();
    {
        let blockstore1 = open_blockstore(&duplicate_leader_ledger_path);
        let blockstore2 = open_blockstore(&target_switch_fork_validator_ledger_path);
        copy_blocks(dup_slot, &blockstore1, &blockstore2);
    }
    clear_ledger_and_tower(
        &target_switch_fork_validator_ledger_path,
        &target_switch_fork_validator_pubkey,
        dup_slot,
    );

    info!(
        "Purging towers and ledgers for: {:?}",
        duplicate_fork_validator1_pubkey
    );
    clear_ledger_and_tower(
        &duplicate_fork_validator1_ledger_path,
        &duplicate_fork_validator1_pubkey,
        dup_slot + 1,
    );

    info!(
        "Purging towers and ledgers for: {:?}",
        duplicate_fork_validator2_pubkey
    );
    // Copy validator 1's ledger to validator 2 so that they have the same version
    // of the duplicate slot
    clear_ledger_and_tower(
        &duplicate_fork_validator2_ledger_path,
        &duplicate_fork_validator2_pubkey,
        dup_slot,
    );
    Blockstore::destroy(&duplicate_fork_validator2_ledger_path).unwrap();
    {
        let blockstore1 = open_blockstore(&duplicate_fork_validator1_ledger_path);
        let blockstore2 = open_blockstore(&duplicate_fork_validator2_ledger_path);
        copy_blocks(dup_slot, &blockstore1, &blockstore2);
    }

    // Set entrypoint to `target_switch_fork_validator_pubkey` so we can run discovery in gossip even without the
    // bad leader
    cluster.set_entry_point(target_switch_fork_validator_info.info.contact_info.clone());

    // 4) Restart `target_switch_fork_validator_pubkey`, and ensure they vote on their own leader slot
    // that's not descended from the duplicate slot
    info!("Restarting switch fork node");
    target_switch_fork_validator_info.config.voting_disabled = false;
    cluster.restart_node(
        &target_switch_fork_validator_pubkey,
        target_switch_fork_validator_info,
        SocketAddrSpace::Unspecified,
    );
    let target_switch_fork_validator_ledger_path =
        cluster.ledger_path(&target_switch_fork_validator_pubkey);

    info!("Waiting for switch fork to make block past duplicate fork");
    loop {
        let last_vote = wait_for_last_vote_in_tower_to_land_in_ledger(
            &target_switch_fork_validator_ledger_path,
            &target_switch_fork_validator_pubkey,
        );
        if let Some(latest_vote_slot) = last_vote {
            if latest_vote_slot > dup_slot {
                let blockstore = open_blockstore(&target_switch_fork_validator_ledger_path);
                let ancestor_slots: HashSet<Slot> =
                    AncestorIterator::new_inclusive(latest_vote_slot, &blockstore).collect();
                assert!(ancestor_slots.contains(&latest_vote_slot));
                assert!(ancestor_slots.contains(&0));
                assert!(!ancestor_slots.contains(&dup_slot));
                break;
            }
        }
        sleep(Duration::from_millis(1000));
    }

    // Now restart the duplicate validators
    // Start the node with partition enabled so they don't see the `target_switch_fork_validator_pubkey`
    // before voting on the duplicate block
    info!("Restarting duplicate fork node");
    // Ensure `duplicate_fork_validator1_pubkey` votes before starting up `duplicate_fork_validator2_pubkey`
    // to prevent them seeing `dup_slot` as duplicate confirmed before voting.
    restart_dup_validator(
        &mut cluster,
        duplicate_fork_validator1_info,
        &duplicate_fork_validator1_pubkey,
        dup_slot,
        &dup_shred1,
        &dup_shred2,
    );
    restart_dup_validator(
        &mut cluster,
        duplicate_fork_validator2_info,
        &duplicate_fork_validator2_pubkey,
        dup_slot,
        &dup_shred1,
        &dup_shred2,
    );

    // Wait for the `duplicate_fork_validator1_pubkey` to make another leader block on top
    // of the duplicate fork which includes their own vote for `dup_block`. This
    // should make the duplicate fork the heaviest
    info!("Waiting on duplicate fork validator to generate block on top of duplicate fork",);
    loop {
        let duplicate_fork_validator_blockstore =
            open_blockstore(&cluster.ledger_path(&duplicate_fork_validator1_pubkey));
        let meta = duplicate_fork_validator_blockstore
            .meta(dup_slot)
            .unwrap()
            .unwrap();
        if !meta.next_slots.is_empty() {
            info!(
                "duplicate fork validator saw new slots: {:?} on top of duplicate slot",
                meta.next_slots
            );
            break;
        }
        sleep(Duration::from_millis(1000));
    }

    // Check that the cluster is making progress
    cluster.check_for_new_roots(
        16,
        "test_duplicate_shreds_switch_failure",
        SocketAddrSpace::Unspecified,
    );
}

/// Forks previous marked invalid should be marked as such in fork choice on restart
#[test]
#[serial]
fn test_invalid_forks_persisted_on_restart() {
    solana_logger::setup_with("info,solana_metrics=off,solana_ledger=off");

    let dup_slot = 10;
    let validator_keypairs = [
        "28bN3xyvrP4E8LwEgtLjhnkb7cY4amQb6DrYAbAYjgRV4GAGgkVM2K7wnxnAS7WDneuavza7x21MiafLu1HkwQt4",
        "2saHBBoTkLMmttmPQP8KfBkcCw45S5cwtV3wTdGCscRC8uxdgvHxpHiWXKx4LvJjNJtnNcbSv5NdheokFFqnNDt8",
    ]
    .iter()
    .map(|s| (Arc::new(Keypair::from_base58_string(s)), true))
    .collect::<Vec<_>>();
    let majority_keypair = validator_keypairs[1].0.clone();

    let validators = validator_keypairs
        .iter()
        .map(|(kp, _)| kp.pubkey())
        .collect::<Vec<_>>();

    let node_stakes = vec![DEFAULT_NODE_STAKE, 100 * DEFAULT_NODE_STAKE];
    let (target_pubkey, majority_pubkey) = (validators[0], validators[1]);
    // Need majority validator to make the dup_slot
    let validator_to_slots = vec![
        (majority_pubkey, dup_slot as usize + 5),
        (target_pubkey, DEFAULT_SLOTS_PER_EPOCH as usize),
    ];
    let leader_schedule = create_custom_leader_schedule(validator_to_slots.into_iter());
    let mut default_config = ValidatorConfig::default_for_test();
    default_config.fixed_leader_schedule = Some(FixedSchedule {
        leader_schedule: Arc::new(leader_schedule),
    });
    let mut validator_configs = make_identical_validator_configs(&default_config, 2);
    // Majority shouldn't duplicate confirm anything
    validator_configs[1].voting_disabled = true;

    let mut cluster = LocalCluster::new(
        &mut ClusterConfig {
            cluster_lamports: DEFAULT_CLUSTER_LAMPORTS + node_stakes.iter().sum::<u64>(),
            validator_configs,
            node_stakes,
            validator_keys: Some(validator_keypairs),
            skip_warmup_slots: true,
            ..ClusterConfig::default()
        },
        SocketAddrSpace::Unspecified,
    );

    let target_ledger_path = cluster.ledger_path(&target_pubkey);

    // Wait for us to vote past duplicate slot
    let timer = Instant::now();
    loop {
        if let Some(slot) =
            wait_for_last_vote_in_tower_to_land_in_ledger(&target_ledger_path, &target_pubkey)
        {
            if slot > dup_slot {
                break;
            }
        }

        assert!(
            timer.elapsed() < Duration::from_secs(30),
            "Did not make more than 10 blocks in 30 seconds"
        );
        sleep(Duration::from_millis(100));
    }

    // Send duplicate
    let parent = {
        let blockstore = open_blockstore(&target_ledger_path);
        let parent = blockstore
            .meta(dup_slot)
            .unwrap()
            .unwrap()
            .parent_slot
            .unwrap();

        let entries = create_ticks(
            64 * (std::cmp::max(1, dup_slot - parent)),
            0,
            cluster.genesis_config.hash(),
        );
        let last_hash = entries.last().unwrap().hash;
        let version = solana_sdk::shred_version::version_from_hash(&last_hash);
        let dup_shreds = Shredder::new(dup_slot, parent, 0, version)
            .unwrap()
            .entries_to_shreds(
                &majority_keypair,
                &entries,
                true,  // is_full_slot
                0,     // next_shred_index,
                0,     // next_code_index
                false, // merkle_variant
                &ReedSolomonCache::default(),
                &mut ProcessShredsStats::default(),
            )
            .0;

        info!("Sending duplicate shreds for {dup_slot}");
        cluster.send_shreds_to_validator(dup_shreds.iter().collect(), &target_pubkey);
        wait_for_duplicate_proof(&target_ledger_path, dup_slot)
            .expect("Duplicate proof for {dup_slot} not found");
        parent
    };

    info!("Duplicate proof for {dup_slot} has landed, restarting node");
    let info = cluster.exit_node(&target_pubkey);

    {
        let blockstore = open_blockstore(&target_ledger_path);
        purge_slots_with_count(&blockstore, dup_slot + 5, 100);
    }

    // Restart, should create an entirely new fork
    cluster.restart_node(&target_pubkey, info, SocketAddrSpace::Unspecified);

    info!("Waiting for fork built off {parent}");
    let timer = Instant::now();
    let mut checked_children: HashSet<Slot> = HashSet::default();
    let mut done = false;
    while !done {
        let blockstore = open_blockstore(&target_ledger_path);
        let parent_meta = blockstore.meta(parent).unwrap().expect("Meta must exist");
        for child in parent_meta.next_slots {
            if checked_children.contains(&child) {
                continue;
            }

            if blockstore.is_full(child) {
                let shreds = blockstore
                    .get_data_shreds_for_slot(child, 0)
                    .expect("Child is full");
                let mut is_our_block = true;
                for shred in shreds {
                    is_our_block &= shred.verify(&target_pubkey);
                }
                if is_our_block {
                    done = true;
                }
                checked_children.insert(child);
            }
        }

        assert!(
            timer.elapsed() < Duration::from_secs(30),
            "Did not create a new fork off parent {parent} in 30 seconds after restart"
        );
        sleep(Duration::from_millis(100));
    }
}

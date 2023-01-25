#![allow(clippy::integer_arithmetic)]
use {
    assert_matches::assert_matches,
    common::*,
    crossbeam_channel::{unbounded, Receiver},
    gag::BufferRedirect,
    log::*,
    serial_test::serial,
    solana_client::thin_client::ThinClient,
    solana_core::{
        broadcast_stage::BroadcastStageType,
        consensus::{Tower, SWITCH_FORK_THRESHOLD, VOTE_THRESHOLD_DEPTH},
        optimistic_confirmation_verifier::OptimisticConfirmationVerifier,
        replay_stage::DUPLICATE_THRESHOLD,
        tower_storage::FileTowerStorage,
        validator::ValidatorConfig,
    },
    solana_download_utils::download_snapshot_archive,
    solana_gossip::gossip_service::discover_cluster,
    solana_ledger::{
        ancestor_iterator::AncestorIterator, bank_forks_utils, blockstore::Blockstore,
        blockstore_processor::ProcessOptions,
    },
    solana_local_cluster::{
        cluster::{Cluster, ClusterValidatorInfo},
        cluster_tests,
        local_cluster::{ClusterConfig, LocalCluster},
        validator_configs::*,
    },
    solana_pubsub_client::pubsub_client::PubsubClient,
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_api::{
        config::{RpcProgramAccountsConfig, RpcSignatureSubscribeConfig},
        response::RpcSignatureResult,
    },
    solana_runtime::{
        hardened_unpack::open_genesis_config,
        snapshot_archive_info::SnapshotArchiveInfoGetter,
        snapshot_config::SnapshotConfig,
        snapshot_package::SnapshotType,
        snapshot_utils::{self, ArchiveFormat, SnapshotVersion},
    },
    solana_sdk::{
        account::AccountSharedData,
        client::{AsyncClient, SyncClient},
        clock::{self, Slot, DEFAULT_TICKS_PER_SLOT, MAX_PROCESSING_AGE},
        commitment_config::CommitmentConfig,
        epoch_schedule::MINIMUM_SLOTS_PER_EPOCH,
        genesis_config::ClusterType,
        hard_forks::HardForks,
        poh_config::PohConfig,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_program, system_transaction,
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_vote_program::vote_state::MAX_LOCKOUT_HISTORY,
    std::{
        collections::{HashMap, HashSet},
        fs,
        io::Read,
        iter,
        path::Path,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        thread::{sleep, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

mod common;

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
        .find(|id| *id != cluster.entry_point_info.id)
        .unwrap();
    let non_bootstrap_info = cluster.get_contact_info(&non_bootstrap_id).unwrap();

    let (rpc, tpu) = cluster_tests::get_client_facing_addr(non_bootstrap_info);
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
    let leader_pubkey = cluster.entry_point_info.id;
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
        &cluster.entry_point_info,
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
        &cluster.entry_point_info.gossip,
        1,
        SocketAddrSpace::Unspecified,
    )
    .unwrap();
    assert_eq!(cluster_nodes.len(), 1);

    let (rpc, tpu) = cluster_tests::get_client_facing_addr(&cluster.entry_point_info);
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
        &cluster.entry_point_info.rpc,
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
        SnapshotType::FullSnapshot,
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
        &cluster.entry_point_info.rpc,
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
        SnapshotType::FullSnapshot,
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
        &cluster.entry_point_info.rpc,
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
        SnapshotType::IncrementalSnapshot(incremental_snapshot_archive_info.base_slot()),
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
        &cluster.entry_point_info.rpc,
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .path(),
        validator_snapshot_test_config
            .incremental_snapshot_archives_dir
            .path(),
        (full_snapshot_archive.slot(), *full_snapshot_archive.hash()),
        SnapshotType::FullSnapshot,
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
        &cluster.entry_point_info.rpc,
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
        SnapshotType::IncrementalSnapshot(incremental_snapshot_archive.base_slot()),
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
    let validator_info = cluster.exit_node(&validator_identity.pubkey());
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

    // Copy over the snapshots to the new node, but need to remove the tmp snapshot dir so it
    // doesn't break the simple copy_files closure.
    snapshot_utils::remove_tmp_snapshot_archives(
        validator_snapshot_test_config
            .full_snapshot_archives_dir
            .path(),
    );
    snapshot_utils::remove_tmp_snapshot_archives(
        validator_snapshot_test_config
            .incremental_snapshot_archives_dir
            .path(),
    );
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
        .find(|x| *x != cluster.entry_point_info.id)
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
        ArchiveFormat::TarBzip2,
    );
    fs::hard_link(archive_info.path(), validator_archive_path).unwrap();
    let slot_floor = archive_info.slot();

    // Start up a new node from a snapshot
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
        DEFAULT_NODE_STAKE,
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
            &cluster.entry_point_info,
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
fn test_wait_for_max_stake() {
    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let validator_config = ValidatorConfig::default_for_test();
    let mut config = ClusterConfig {
        cluster_lamports: DEFAULT_CLUSTER_LAMPORTS,
        node_stakes: vec![DEFAULT_NODE_STAKE; 4],
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
    let node_stakes = vec![50 * DEFAULT_NODE_STAKE, 51 * DEFAULT_NODE_STAKE];
    let validator_keys: Vec<_> = vec![
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
        // 2) Won't reset to this earlier ancestor becasue reset can only happen on same voted fork if
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
        poh_verify: false,
        ..ProcessOptions::default()
    };
    let ledger_path = blockstore.ledger_path();
    let genesis_config = open_genesis_config(ledger_path, u64::max_value());
    let snapshot_config = Some(create_simple_snapshot_config(ledger_path));
    let (bank_forks, ..) = bank_forks_utils::load(
        &genesis_config,
        blockstore,
        vec![ledger_path.join("accounts")],
        None,
        snapshot_config.as_ref(),
        process_options,
        None,
        None,
        None,
        &Arc::default(),
    )
    .unwrap();
    let bank = bank_forks.read().unwrap().get(snapshot_slot).unwrap();
    let full_snapshot_archive_info = snapshot_utils::bank_to_full_snapshot_archive(
        ledger_path,
        &bank,
        Some(SnapshotVersion::default()),
        ledger_path,
        ledger_path,
        ArchiveFormat::TarZstd,
        1,
        1,
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

    let validator_strings = vec![
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
        additional_accounts,
    );
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
    let node_stakes = vec![51 * DEFAULT_NODE_STAKE, 50 * DEFAULT_NODE_STAKE];
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

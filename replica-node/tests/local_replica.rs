#![allow(clippy::integer_arithmetic)]
use log::*;
use serial_test::serial;

use solana_core::validator::ValidatorConfig;

use solana_gossip::{
    cluster_info::{Node, VALIDATOR_PORT_RANGE},
    cluster_info::ClusterInfo,
    contact_info::ContactInfo,
};

use solana_local_cluster::{
    cluster::Cluster,
    local_cluster::{ClusterConfig, LocalCluster},
    validator_configs::*,
};
use solana_replica_node::replica_util;

use solana_runtime::{
    snapshot_config::SnapshotConfig,
    snapshot_utils::{self, ArchiveFormat},
};
use solana_sdk::{
    client::SyncClient, clock::Slot, commitment_config::CommitmentConfig,
    epoch_schedule::MINIMUM_SLOTS_PER_EPOCH, hash::Hash,
    signature::{Keypair, Signer},
};

use std::{
    net::{IpAddr, SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    sync::Arc,
    thread::sleep,
    time::Duration,
};
use tempfile::TempDir;

use solana_replica_node::replica_node::{ReplicaNode, ReplicaNodeConfig};

const RUST_LOG_FILTER: &str =
    "error,solana_core::replay_stage=warn,solana_local_cluster=info,local_cluster=info";

fn wait_for_next_snapshot(
    cluster: &LocalCluster,
    snapshot_package_output_path: &Path,
) -> (PathBuf, (Slot, Hash)) {
    // Get slot after which this was generated
    let client = cluster
        .get_validator_client(&cluster.entry_point_info.id)
        .unwrap();
    let last_slot = client
        .get_slot_with_commitment(CommitmentConfig::processed())
        .expect("Couldn't get slot");

    // Wait for a snapshot for a bank >= last_slot to be made so we know that the snapshot
    // must include the transactions just pushed
    trace!(
        "Waiting for snapshot archive to be generated with slot > {}",
        last_slot
    );
    loop {
        if let Some(full_snapshot_archive_info) =
            snapshot_utils::get_highest_full_snapshot_archive_info(snapshot_package_output_path)
        {
            trace!(
                "full snapshot for slot {} exists",
                full_snapshot_archive_info.slot()
            );
            if *full_snapshot_archive_info.slot() >= last_slot {
                return (
                    full_snapshot_archive_info.path().clone(),
                    (
                        *full_snapshot_archive_info.slot(),
                        *full_snapshot_archive_info.hash(),
                    ),
                );
            }
            trace!(
                "full snapshot slot {} < last_slot {}",
                full_snapshot_archive_info.slot(),
                last_slot
            );
        }
        sleep(Duration::from_millis(1000));
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
    snapshot_archives_dir: TempDir,
    account_storage_dirs: Vec<TempDir>,
    validator_config: ValidatorConfig,
}

fn setup_snapshot_validator_config(
    snapshot_interval_slots: u64,
    num_account_paths: usize,
) -> SnapshotValidatorConfig {
    // Create the snapshot config
    let snapshot_dir = tempfile::tempdir_in(farf_dir()).unwrap();
    let snapshot_archives_dir = tempfile::tempdir_in(farf_dir()).unwrap();
    let snapshot_config = SnapshotConfig {
        snapshot_interval_slots,
        snapshot_package_output_path: snapshot_archives_dir.path().to_path_buf(),
        snapshot_path: snapshot_dir.path().to_path_buf(),
        archive_format: ArchiveFormat::TarBzip2,
        snapshot_version: snapshot_utils::SnapshotVersion::default(),
        maximum_snapshots_to_retain: snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
    };

    // Create the account paths
    let (account_storage_dirs, account_storage_paths) = generate_account_paths(num_account_paths);

    // Create the validator config
    let validator_config = ValidatorConfig {
        snapshot_config: Some(snapshot_config),
        account_paths: account_storage_paths,
        accounts_hash_interval_slots: snapshot_interval_slots,
        ..ValidatorConfig::default()
    };

    SnapshotValidatorConfig {
        _snapshot_dir: snapshot_dir,
        snapshot_archives_dir,
        account_storage_dirs,
        validator_config,
    }
}

fn test_local_cluster_start_and_exit_with_config() {
    solana_logger::setup();
    const NUM_NODES: usize = 1;
    let mut config = ClusterConfig {
        validator_configs: make_identical_validator_configs(&ValidatorConfig::default(), NUM_NODES),
        node_stakes: vec![3; NUM_NODES],
        cluster_lamports: 100,
        ticks_per_slot: 8,
        slots_per_epoch: MINIMUM_SLOTS_PER_EPOCH as u64,
        stakers_slot_offset: MINIMUM_SLOTS_PER_EPOCH as u64,
        ..ClusterConfig::default()
    };
    let cluster = LocalCluster::new(&mut config);
    assert_eq!(cluster.validators.len(), NUM_NODES);
}

#[test]
#[serial]
fn test_replica_bootstrap() {
    test_local_cluster_start_and_exit_with_config();

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

    let mut cluster = LocalCluster::new(&mut config);

    assert_eq!(cluster.validators.len(), 1);
    let contact_info = &cluster.validators.values().next().unwrap().info.contact_info;

    println!("Contact info: {:?}", contact_info);

    // Get slot after which this was generated
    let snapshot_package_output_path = &leader_snapshot_test_config
        .validator_config
        .snapshot_config
        .as_ref()
        .unwrap()
        .snapshot_package_output_path;
    info!("Waiting for snapshot");
    let (archive_filename, archive_snapshot_hash) =
        wait_for_next_snapshot(&cluster, snapshot_package_output_path);
    info!("found: {:?}", archive_filename);

    let expected_shred_version = contact_info.shred_version;
    let identity_keypair = Keypair::new();

    // now bring up a replica to talk to it.
    let rpc_addr: SocketAddr = "127.0.0.1:8301".parse().unwrap();
    let rpc_pubsub_addr: SocketAddr = "127.0.0.1:8302".parse().unwrap();
    let ledger_dir = tempfile::tempdir_in(farf_dir()).unwrap();
    let ledger_path = ledger_dir.path();
    let snapshot_output_dir = tempfile::tempdir_in(farf_dir()).unwrap();
    let snapshot_output_path = snapshot_output_dir.path();
    let snapshot_path = snapshot_output_path.join("snapshot");
    let account_paths: Vec<PathBuf> = vec![ledger_path.join("accounts")];

    let gossip_addr: SocketAddr = "127.0.0.1:8303".parse().unwrap();
    let dynamic_port_range = solana_net_utils::parse_port_range("8300-8400").unwrap();
    let bind_address = solana_net_utils::parse_host("127.0.0.1").unwrap();
    let node = Node::new_with_external_ip(
        &identity_keypair.pubkey(),
        &gossip_addr,
        dynamic_port_range,
        bind_address,
    );

    info!("The peer id: {:?}", &contact_info.id);
    let entry_points = vec![ContactInfo::new_gossip_entry_point(&contact_info.gossip)];
    let (cluster_info, rpc_contact_info, snapshot_info) = replica_util::get_rpc_peer_info(
        identity_keypair,
        &entry_points,
        &ledger_path,
        &node,
        None,
        &contact_info.id,
        &snapshot_output_path,
    );

    info!("The cluster info:\n{:?}", cluster_info.contact_info_trace());

    let config = ReplicaNodeConfig {
        rpc_source_addr: contact_info.rpc,
        rpc_addr,
        rpc_pubsub_addr,
        ledger_path: ledger_path.to_path_buf(),
        snapshot_output_dir: snapshot_output_path.to_path_buf(),
        snapshot_path,
        account_paths,
        snapshot_info: archive_snapshot_hash,
        cluster_info,
        ..ReplicaNodeConfig::default()
    };
    let _replica_node = ReplicaNode::new(config);
}

#![allow(clippy::integer_arithmetic)]
/// Integration testing for the PostgreSQL plugin


use {
    log::*,
    libloading::{Library, Symbol},
    serial_test::serial,
    solana_accountsdb_plugin_postgres,
    solana_core::validator::ValidatorConfig,
    solana_local_cluster::{
        cluster::Cluster,
        local_cluster::{ClusterConfig, LocalCluster},
        validator_configs::*,
    },    
    solana_runtime::{
        accounts_index::AccountSecondaryIndexes, snapshot_archive_info::SnapshotArchiveInfoGetter,
        snapshot_config::SnapshotConfig, snapshot_utils,
    },
    solana_sdk::{
        client::SyncClient,
        clock::Slot,
        commitment_config::CommitmentConfig,
        epoch_schedule::MINIMUM_SLOTS_PER_EPOCH,
        exit::Exit,
        hash::Hash,
        signature::{Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        fs::File,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::{Path, PathBuf},
        sync::{Arc, RwLock},
        thread::sleep,
        time::Duration,
    },    
    tempfile::TempDir,
};

const RUST_LOG_FILTER: &str =
    "error,solana_core::replay_stage=warn,solana_local_cluster=info,local_cluster=info";

fn wait_for_next_snapshot(
    cluster: &LocalCluster,
    snapshot_archives_dir: &Path,
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
            snapshot_utils::get_highest_full_snapshot_archive_info(snapshot_archives_dir)
        {
            trace!(
                "full snapshot for slot {} exists",
                full_snapshot_archive_info.slot()
            );
            if full_snapshot_archive_info.slot() >= last_slot {
                return (
                    full_snapshot_archive_info.path().clone(),
                    (
                        full_snapshot_archive_info.slot(),
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

fn generate_accountsdb_plugin_config() -> (TempDir, File, PathBuf) {
    let tmp_dir = tempfile::tempdir_in(farf_dir()).unwrap();
    let mut path = tmp_dir.path().to_path_buf();
    path.push("accounts_db_plugin.json");
    let config_file = File::create(path.clone()).unwrap();
    (tmp_dir, config_file, path)
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
    let bank_snapshots_dir = tempfile::tempdir_in(farf_dir()).unwrap();
    let snapshot_archives_dir = tempfile::tempdir_in(farf_dir()).unwrap();
    let snapshot_config = SnapshotConfig {
        full_snapshot_archive_interval_slots: snapshot_interval_slots,
        incremental_snapshot_archive_interval_slots: Slot::MAX,
        snapshot_archives_dir: snapshot_archives_dir.path().to_path_buf(),
        bank_snapshots_dir: bank_snapshots_dir.path().to_path_buf(),
        ..SnapshotConfig::default()
    };

    // Create the account paths
    let (account_storage_dirs, account_storage_paths) = generate_account_paths(num_account_paths);

    let bind_ip_addr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    let accountsdb_repl_port =
        solana_net_utils::find_available_port_in_range(bind_ip_addr, (1024, 65535)).unwrap();
    let replica_server_addr = SocketAddr::new(bind_ip_addr, accountsdb_repl_port);

    let (_, config_file, path) = generate_accountsdb_plugin_config();

    let accountsdb_plugin_config_files = Some(
        vec![path]
    );

    // Create the validator config
    let validator_config = ValidatorConfig {
        snapshot_config: Some(snapshot_config),
        account_paths: account_storage_paths,
        accounts_hash_interval_slots: snapshot_interval_slots,
        accountsdb_plugin_config_files,
        ..ValidatorConfig::default()
    };

    SnapshotValidatorConfig {
        _snapshot_dir: bank_snapshots_dir,
        snapshot_archives_dir,
        account_storage_dirs,
        validator_config,
    }
}

fn test_local_cluster_start_and_exit_with_config(socket_addr_space: SocketAddrSpace) {
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
    let cluster = LocalCluster::new(&mut config, socket_addr_space);
    assert_eq!(cluster.validators.len(), NUM_NODES);
}

#[test]
#[serial]
fn test_postgres_plugin() {
    unsafe {
        let lib = Library::new("libsolana_accountsdb_plugin_postgres.so");
        if lib.is_err() {
            info!("Failed to load the dynamic library libsolana_accountsdb_plugin_postgres.so");
            return;
        }
    }

    solana_logger::setup_with_default(RUST_LOG_FILTER);
    let socket_addr_space = SocketAddrSpace::new(true);
    test_local_cluster_start_and_exit_with_config(socket_addr_space);

    // First set up the cluster with 1 node
    let snapshot_interval_slots = 50;
    let num_account_paths = 3;

    let leader_snapshot_test_config =
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
    
        let cluster = LocalCluster::new(&mut config, socket_addr_space);
    
        assert_eq!(cluster.validators.len(), 1);
        let contact_info = &cluster.entry_point_info;
    
        info!("Contact info: {:?}", contact_info);
    
        // Get slot after which this was generated
        let snapshot_archives_dir = &leader_snapshot_test_config
            .validator_config
            .snapshot_config
            .as_ref()
            .unwrap()
            .snapshot_archives_dir;
        info!("Waiting for snapshot");
        let (archive_filename, archive_snapshot_hash) =
            wait_for_next_snapshot(&cluster, snapshot_archives_dir);
        info!("found: {:?}", archive_filename);
    
    }
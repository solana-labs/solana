#![allow(clippy::integer_arithmetic)]
/// Integration testing for the PostgreSQL plugin


use {
    log::*,
    libloading::{Library, Symbol},
    serial_test::serial,
    solana_core::validator::ValidatorConfig,
    solana_accountsdb_plugin_postgres,
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
    // First set up the cluster with 1 node
    let snapshot_interval_slots = 50;
    let num_account_paths = 3;

    let leader_snapshot_test_config =
        setup_snapshot_validator_config(snapshot_interval_slots, num_account_paths);
}
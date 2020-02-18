use crate::{
    bank_forks::SnapshotConfig,
    blockstore::Blockstore,
    blockstore_processor::{self, BlockstoreProcessorResult, ProcessOptions},
    entry::VerifyRecyclers,
    snapshot_utils,
};
use log::*;
use solana_sdk::genesis_config::GenesisConfig;
use std::{fs, path::PathBuf, sync::Arc};

pub fn load(
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    account_paths: Vec<PathBuf>,
    snapshot_config: Option<&SnapshotConfig>,
    process_options: ProcessOptions,
) -> BlockstoreProcessorResult {
    if let Some(snapshot_config) = snapshot_config.as_ref() {
        info!(
            "Initializing snapshot path: {:?}",
            snapshot_config.snapshot_path
        );
        let _ = fs::remove_dir_all(&snapshot_config.snapshot_path);
        fs::create_dir_all(&snapshot_config.snapshot_path)
            .expect("Couldn't create snapshot directory");

        let mut tar = snapshot_utils::get_snapshot_archive_path(
            &snapshot_config.snapshot_package_output_path,
        );
        let fallback_tar = snapshot_utils::get_fallback_snapshot_archive_path(
            &snapshot_config.snapshot_package_output_path,
        );
        let mut snapshot_exists = true;

        if tar.exists() && fallback_tar.exists() {
            panic!(
                "two ambiguous variants of snapshots are available: {:?}, {:?}",
                tar, fallback_tar
            );
        }
        if !tar.exists() {
            if fallback_tar.exists() {
                warn!("using snapshot file ({:?}) as a fallback, because the desired one({:?}) is missing", fallback_tar, tar);
                tar = fallback_tar;
            } else {
                info!(
                    "Snapshot packages don't exist: {:?}, {:?}",
                    &tar, &fallback_tar
                );
                snapshot_exists = false;
            }
        }

        if snapshot_exists {
            info!("Loading snapshot package: {:?}", tar);
            // Fail hard here if snapshot fails to load, don't silently continue

            if account_paths.is_empty() {
                panic!("Account paths not present when booting from snapshot")
            }

            let deserialized_bank = snapshot_utils::bank_from_archive(
                &account_paths,
                &snapshot_config.snapshot_path,
                &tar,
            )
            .expect("Load from snapshot failed");

            return blockstore_processor::process_blockstore_from_root(
                genesis_config,
                blockstore,
                Arc::new(deserialized_bank),
                &process_options,
                &VerifyRecyclers::default(),
            );
        }
    } else {
        info!("Snapshots disabled");
    }

    info!("Processing ledger from genesis");
    blockstore_processor::process_blockstore(
        &genesis_config,
        &blockstore,
        account_paths,
        process_options,
    )
}

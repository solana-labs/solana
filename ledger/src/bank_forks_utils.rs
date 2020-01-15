use crate::{
    bank_forks::{BankForks, SnapshotConfig},
    blockstore::Blockstore,
    blockstore_processor::{self, BankForksInfo, BlockstoreProcessorError, ProcessOptions},
    entry::VerifyRecyclers,
    leader_schedule_cache::LeaderScheduleCache,
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
) -> Result<(BankForks, Vec<BankForksInfo>, LeaderScheduleCache), BlockstoreProcessorError> {
    if let Some(snapshot_config) = snapshot_config.as_ref() {
        info!(
            "Initializing snapshot path: {:?}",
            snapshot_config.snapshot_path
        );
        let _ = fs::remove_dir_all(&snapshot_config.snapshot_path);
        fs::create_dir_all(&snapshot_config.snapshot_path)
            .expect("Couldn't create snapshot directory");

        let tar =
            snapshot_utils::get_snapshot_tar_path(&snapshot_config.snapshot_package_output_path);
        if tar.exists() {
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
        } else {
            info!("Snapshot package does not exist: {:?}", tar);
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

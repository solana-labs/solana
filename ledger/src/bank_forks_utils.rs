use crate::{
    bank_forks::{BankForks, SnapshotConfig},
    blocktree::Blocktree,
    blocktree_processor::{self, BankForksInfo, BlocktreeProcessorError, ProcessOptions},
    leader_schedule_cache::LeaderScheduleCache,
    snapshot_utils,
};
use log::*;
use solana_sdk::genesis_block::GenesisBlock;
use std::{fs, sync::Arc};

pub fn load(
    genesis_block: &GenesisBlock,
    blocktree: &Blocktree,
    account_paths: Option<String>,
    snapshot_config: Option<&SnapshotConfig>,
    process_options: ProcessOptions,
) -> Result<(BankForks, Vec<BankForksInfo>, LeaderScheduleCache), BlocktreeProcessorError> {
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
            let deserialized_bank = snapshot_utils::bank_from_archive(
                account_paths
                    .clone()
                    .expect("Account paths not present when booting from snapshot"),
                &snapshot_config.snapshot_path,
                &tar,
            )
            .expect("Load from snapshot failed");

            return blocktree_processor::process_blocktree_from_root(
                genesis_block,
                blocktree,
                Arc::new(deserialized_bank),
                &process_options,
            );
        } else {
            info!("Snapshot package does not exist: {:?}", tar);
        }
    } else {
        info!("Snapshots disabled");
    }

    info!("Processing ledger from genesis");
    blocktree_processor::process_blocktree(
        &genesis_block,
        &blocktree,
        account_paths,
        process_options,
    )
}

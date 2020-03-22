use crate::{
    bank_forks::{BankForks, SnapshotConfig},
    blockstore::Blockstore,
    blockstore_processor::{
        self, BankForksInfo, BlockstoreProcessorError, BlockstoreProcessorResult, ProcessOptions,
    },
    entry::VerifyRecyclers,
    leader_schedule_cache::LeaderScheduleCache,
    snapshot_utils,
};
use log::*;
use solana_sdk::{clock::Slot, genesis_config::GenesisConfig, hash::Hash};
use std::{fs, path::PathBuf, process, result, sync::Arc};

pub type LoadResult = result::Result<
    (
        BankForks,
        Vec<BankForksInfo>,
        LeaderScheduleCache,
        Option<(Slot, Hash)>,
    ),
    BlockstoreProcessorError,
>;

fn to_loadresult(
    brp: BlockstoreProcessorResult,
    snapshot_hash: Option<(Slot, Hash)>,
) -> LoadResult {
    brp.map(|(bank_forks, bank_forks_info, leader_schedule_cache)| {
        (
            bank_forks,
            bank_forks_info,
            leader_schedule_cache,
            snapshot_hash,
        )
    })
}

pub fn load(
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    account_paths: Vec<PathBuf>,
    snapshot_config: Option<&SnapshotConfig>,
    process_options: ProcessOptions,
) -> LoadResult {
    if let Some(snapshot_config) = snapshot_config.as_ref() {
        info!(
            "Initializing snapshot path: {:?}",
            snapshot_config.snapshot_path
        );
        let _ = fs::remove_dir_all(&snapshot_config.snapshot_path);
        fs::create_dir_all(&snapshot_config.snapshot_path)
            .expect("Couldn't create snapshot directory");

        match snapshot_utils::get_highest_snapshot_archive_path(
            &snapshot_config.snapshot_package_output_path,
        ) {
            Some((archive_filename, archive_snapshot_hash)) => {
                info!("Loading snapshot package: {:?}", archive_filename);
                // Fail hard here if snapshot fails to load, don't silently continue

                if account_paths.is_empty() {
                    error!("Account paths not present when booting from snapshot");
                    process::exit(1);
                }

                let deserialized_bank = snapshot_utils::bank_from_archive(
                    &account_paths,
                    &process_options.frozen_accounts,
                    &snapshot_config.snapshot_path,
                    &archive_filename,
                )
                .expect("Load from snapshot failed");

                let deserialized_snapshot_hash = (
                    deserialized_bank.slot(),
                    deserialized_bank.get_accounts_hash(),
                );

                if deserialized_snapshot_hash != archive_snapshot_hash {
                    error!(
                        "Snapshot has mismatch:\narchive: {:?}\ndeserialized: {:?}",
                        archive_snapshot_hash, deserialized_snapshot_hash
                    );
                    process::exit(1);
                }

                return to_loadresult(
                    blockstore_processor::process_blockstore_from_root(
                        genesis_config,
                        blockstore,
                        Arc::new(deserialized_bank),
                        &process_options,
                        &VerifyRecyclers::default(),
                    ),
                    Some(deserialized_snapshot_hash),
                );
            }
            None => info!("No snapshot package available"),
        }
    } else {
        info!("Snapshots disabled");
    }

    info!("Processing ledger from genesis");
    to_loadresult(
        blockstore_processor::process_blockstore(
            &genesis_config,
            &blockstore,
            account_paths,
            process_options,
        ),
        None,
    )
}

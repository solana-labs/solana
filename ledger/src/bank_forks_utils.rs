use crate::{
    blockstore::Blockstore,
    blockstore_processor::{
        self, BlockstoreProcessorError, BlockstoreProcessorResult, CacheBlockTimeSender,
        ProcessOptions, TransactionStatusSender,
    },
    entry::VerifyRecyclers,
    leader_schedule_cache::LeaderScheduleCache,
};
use log::*;
use solana_runtime::{
    bank_forks::{BankForks, SnapshotConfig},
    snapshot_utils,
};
use solana_sdk::{clock::Slot, genesis_config::GenesisConfig, hash::Hash};
use std::{fs, path::PathBuf, process, result};

pub type LoadResult = result::Result<
    (BankForks, LeaderScheduleCache, Option<(Slot, Hash)>),
    BlockstoreProcessorError,
>;

fn to_loadresult(
    brp: BlockstoreProcessorResult,
    snapshot_hash: Option<(Slot, Hash)>,
) -> LoadResult {
    brp.map(|(bank_forks, leader_schedule_cache)| {
        (bank_forks, leader_schedule_cache, snapshot_hash)
    })
}

pub fn load(
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    account_paths: Vec<PathBuf>,
    shrink_paths: Option<Vec<PathBuf>>,
    snapshot_config: Option<&SnapshotConfig>,
    process_options: ProcessOptions,
    transaction_status_sender: Option<&TransactionStatusSender>,
    cache_block_time_sender: Option<&CacheBlockTimeSender>,
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
            Some((archive_filename, (archive_slot, archive_snapshot_hash, compression))) => {
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
                    compression,
                    genesis_config,
                    process_options.debug_keys.clone(),
                    Some(&crate::builtins::get(process_options.bpf_jit)),
                    process_options.account_indexes.clone(),
                    process_options.accounts_db_caching_enabled,
                )
                .expect("Load from snapshot failed");
                if let Some(shrink_paths) = shrink_paths {
                    deserialized_bank.set_shrink_paths(shrink_paths);
                }

                let deserialized_snapshot_hash = (
                    deserialized_bank.slot(),
                    deserialized_bank.get_accounts_hash(),
                );

                if deserialized_snapshot_hash != (archive_slot, archive_snapshot_hash) {
                    error!(
                        "Snapshot has mismatch:\narchive: {:?}\ndeserialized: {:?}",
                        archive_snapshot_hash, deserialized_snapshot_hash
                    );
                    process::exit(1);
                }

                return to_loadresult(
                    blockstore_processor::process_blockstore_from_root(
                        blockstore,
                        deserialized_bank,
                        &process_options,
                        &VerifyRecyclers::default(),
                        transaction_status_sender,
                        cache_block_time_sender,
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
            cache_block_time_sender,
        ),
        None,
    )
}

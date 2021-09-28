use crate::{
    blockstore::Blockstore,
    blockstore_processor::{
        self, BlockstoreProcessorError, BlockstoreProcessorResult, CacheBlockMetaSender,
        ProcessOptions, TransactionStatusSender,
    },
    leader_schedule_cache::LeaderScheduleCache,
};

use log::*;
use solana_entry::entry::VerifyRecyclers;
use solana_runtime::{
    accounts_update_notifier_interface::AccountsUpdateNotifier, bank_forks::BankForks,
    snapshot_archive_info::SnapshotArchiveInfoGetter, snapshot_config::SnapshotConfig,
    snapshot_package::AccountsPackageSender, snapshot_utils,
};
use solana_sdk::{clock::Slot, genesis_config::GenesisConfig, hash::Hash};
use std::{fs, path::PathBuf, process, result};

pub type LoadResult = result::Result<
    (
        BankForks,
        LeaderScheduleCache,
        Option<Slot>,
        Option<(Slot, Hash)>,
    ),
    BlockstoreProcessorError,
>;

fn to_loadresult(
    bpr: BlockstoreProcessorResult,
    snapshot_slot_and_hash: Option<(Slot, Hash)>,
) -> LoadResult {
    bpr.map(
        |(bank_forks, leader_schedule_cache, last_full_snapshot_slot)| {
            (
                bank_forks,
                leader_schedule_cache,
                last_full_snapshot_slot,
                snapshot_slot_and_hash,
            )
        },
    )
}

/// Load the banks and accounts
///
/// If a snapshot config is given, and a snapshot is found, it will be loaded.  Otherwise, load
/// from genesis.
#[allow(clippy::too_many_arguments)]
pub fn load(
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    account_paths: Vec<PathBuf>,
    shrink_paths: Option<Vec<PathBuf>>,
    snapshot_config: Option<&SnapshotConfig>,
    process_options: ProcessOptions,
    transaction_status_sender: Option<&TransactionStatusSender>,
    cache_block_meta_sender: Option<&CacheBlockMetaSender>,
    accounts_package_sender: AccountsPackageSender,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
) -> LoadResult {
    if let Some(snapshot_config) = snapshot_config {
        info!(
            "Initializing bank snapshot path: {}",
            snapshot_config.bank_snapshots_dir.display()
        );
        let _ = fs::remove_dir_all(&snapshot_config.bank_snapshots_dir);
        fs::create_dir_all(&snapshot_config.bank_snapshots_dir)
            .expect("Couldn't create snapshot directory");

        if snapshot_utils::get_highest_full_snapshot_archive_info(
            &snapshot_config.snapshot_archives_dir,
        )
        .is_some()
        {
            return load_from_snapshot(
                genesis_config,
                blockstore,
                account_paths,
                shrink_paths,
                snapshot_config,
                process_options,
                transaction_status_sender,
                cache_block_meta_sender,
                accounts_package_sender,
                accounts_update_notifier,
            );
        } else {
            info!("No snapshot package available; will load from genesis");
        }
    } else {
        info!("Snapshots disabled; will load from genesis");
    }

    load_from_genesis(
        genesis_config,
        blockstore,
        account_paths,
        process_options,
        cache_block_meta_sender,
        snapshot_config,
        accounts_package_sender,
        accounts_update_notifier,
    )
}

fn load_from_genesis(
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    account_paths: Vec<PathBuf>,
    process_options: ProcessOptions,
    cache_block_meta_sender: Option<&CacheBlockMetaSender>,
    snapshot_config: Option<&SnapshotConfig>,
    accounts_package_sender: AccountsPackageSender,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
) -> LoadResult {
    info!("Processing ledger from genesis");
    to_loadresult(
        blockstore_processor::process_blockstore(
            genesis_config,
            blockstore,
            account_paths,
            process_options,
            cache_block_meta_sender,
            snapshot_config,
            accounts_package_sender,
            accounts_update_notifier,
        ),
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn load_from_snapshot(
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    account_paths: Vec<PathBuf>,
    shrink_paths: Option<Vec<PathBuf>>,
    snapshot_config: &SnapshotConfig,
    process_options: ProcessOptions,
    transaction_status_sender: Option<&TransactionStatusSender>,
    cache_block_meta_sender: Option<&CacheBlockMetaSender>,
    accounts_package_sender: AccountsPackageSender,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
) -> LoadResult {
    // Fail hard here if snapshot fails to load, don't silently continue
    if account_paths.is_empty() {
        error!("Account paths not present when booting from snapshot");
        process::exit(1);
    }

    let (deserialized_bank, timings, full_snapshot_archive_info, _) =
        snapshot_utils::bank_from_latest_snapshot_archives(
            &snapshot_config.bank_snapshots_dir,
            &snapshot_config.snapshot_archives_dir,
            &account_paths,
            &process_options.frozen_accounts,
            genesis_config,
            process_options.debug_keys.clone(),
            Some(&crate::builtins::get(process_options.bpf_jit)),
            process_options.account_indexes.clone(),
            process_options.accounts_db_caching_enabled,
            process_options.limit_load_slot_count_from_snapshot,
            process_options.shrink_ratio,
            process_options.accounts_db_test_hash_calculation,
            process_options.accounts_db_skip_shrink,
            process_options.verify_index,
            process_options.accounts_db_config.clone(),
            accounts_update_notifier,
        )
        .expect("Load from snapshot failed");

    let deserialized_bank_slot_and_hash = (
        deserialized_bank.slot(),
        deserialized_bank.get_accounts_hash(),
    );

    if let Some(shrink_paths) = shrink_paths {
        deserialized_bank.set_shrink_paths(shrink_paths);
    }

    to_loadresult(
        blockstore_processor::process_blockstore_from_root(
            blockstore,
            deserialized_bank,
            &process_options,
            &VerifyRecyclers::default(),
            transaction_status_sender,
            cache_block_meta_sender,
            Some(snapshot_config),
            accounts_package_sender,
            timings,
            full_snapshot_archive_info.slot(),
        ),
        Some(deserialized_bank_slot_and_hash),
    )
}

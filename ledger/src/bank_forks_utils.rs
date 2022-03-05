use {
    crate::{
        blockstore::Blockstore,
        blockstore_processor::{
            self, BlockstoreProcessorError, CacheBlockMetaSender, ProcessOptions,
            TransactionStatusSender,
        },
        leader_schedule_cache::LeaderScheduleCache,
    },
    log::*,
    solana_runtime::{
        accounts_update_notifier_interface::AccountsUpdateNotifier,
        bank_forks::BankForks,
        snapshot_archive_info::SnapshotArchiveInfoGetter,
        snapshot_config::SnapshotConfig,
        snapshot_hash::{FullSnapshotHash, IncrementalSnapshotHash, StartingSnapshotHashes},
        snapshot_package::AccountsPackageSender,
        snapshot_utils,
    },
    solana_sdk::{clock::Slot, genesis_config::GenesisConfig},
    std::{fs, path::PathBuf, process, result},
};

pub type LoadResult = result::Result<
    (
        BankForks,
        LeaderScheduleCache,
        Option<Slot>,
        Option<StartingSnapshotHashes>,
    ),
    BlockstoreProcessorError,
>;

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
    let snapshot_present = if let Some(snapshot_config) = snapshot_config {
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
            true
        } else {
            info!("No snapshot package available; will load from genesis");
            false
        }
    } else {
        info!("Snapshots disabled; will load from genesis");
        false
    };

    let (bank_forks, starting_snapshot_hashes) = if snapshot_present {
        bank_forks_from_snapshot(
            genesis_config,
            account_paths,
            shrink_paths,
            snapshot_config.as_ref().unwrap(),
            &process_options,
            accounts_update_notifier,
        )
    } else {
        if process_options
            .accounts_db_config
            .as_ref()
            .and_then(|config| config.filler_account_count)
            .unwrap_or_default()
            > 0
        {
            panic!("filler accounts specified, but not loading from snapshot");
        }

        info!("Processing ledger from genesis");
        (
            blockstore_processor::process_blockstore_for_bank_0(
                genesis_config,
                blockstore,
                account_paths,
                &process_options,
                cache_block_meta_sender,
                accounts_update_notifier,
            ),
            None,
        )
    };

    blockstore_processor::process_blockstore_from_root(
        blockstore,
        bank_forks,
        &process_options,
        transaction_status_sender,
        cache_block_meta_sender,
        snapshot_config,
        accounts_package_sender,
    )
    .map(
        |(bank_forks, leader_schedule_cache, last_full_snapshot_slot)| {
            let last_full_snapshot_slot =
                last_full_snapshot_slot.or_else(|| starting_snapshot_hashes.map(|x| x.full.hash.0));
            (
                bank_forks,
                leader_schedule_cache,
                last_full_snapshot_slot,
                starting_snapshot_hashes,
            )
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn bank_forks_from_snapshot(
    genesis_config: &GenesisConfig,
    account_paths: Vec<PathBuf>,
    shrink_paths: Option<Vec<PathBuf>>,
    snapshot_config: &SnapshotConfig,
    process_options: &ProcessOptions,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
) -> (BankForks, Option<StartingSnapshotHashes>) {
    // Fail hard here if snapshot fails to load, don't silently continue
    if account_paths.is_empty() {
        error!("Account paths not present when booting from snapshot");
        process::exit(1);
    }

    let (deserialized_bank, full_snapshot_archive_info, incremental_snapshot_archive_info) =
        snapshot_utils::bank_from_latest_snapshot_archives(
            &snapshot_config.bank_snapshots_dir,
            &snapshot_config.snapshot_archives_dir,
            &account_paths,
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

    if let Some(shrink_paths) = shrink_paths {
        deserialized_bank.set_shrink_paths(shrink_paths);
    }

    let full_snapshot_hash = FullSnapshotHash {
        hash: (
            full_snapshot_archive_info.slot(),
            *full_snapshot_archive_info.hash(),
        ),
    };
    let starting_incremental_snapshot_hash =
        incremental_snapshot_archive_info.map(|incremental_snapshot_archive_info| {
            IncrementalSnapshotHash {
                base: full_snapshot_hash.hash,
                hash: (
                    incremental_snapshot_archive_info.slot(),
                    *incremental_snapshot_archive_info.hash(),
                ),
            }
        });
    let starting_snapshot_hashes = StartingSnapshotHashes {
        full: full_snapshot_hash,
        incremental: starting_incremental_snapshot_hash,
    };

    (
        BankForks::new(deserialized_bank),
        Some(starting_snapshot_hashes),
    )
}

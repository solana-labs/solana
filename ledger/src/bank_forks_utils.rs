use {
    crate::{
        blockstore::Blockstore,
        blockstore_processor::{
            self, BlockstoreProcessorError, CacheBlockMetaSender, ProcessOptions,
            TransactionStatusSender,
        },
        entry_notifier_service::EntryNotifierSender,
        leader_schedule_cache::LeaderScheduleCache,
        use_snapshot_archives_at_startup::{self, UseSnapshotArchivesAtStartup},
    },
    log::*,
    solana_accounts_db::accounts_update_notifier_interface::AccountsUpdateNotifier,
    solana_runtime::{
        accounts_background_service::AbsRequestSender,
        bank_forks::BankForks,
        snapshot_archive_info::{
            FullSnapshotArchiveInfo, IncrementalSnapshotArchiveInfo, SnapshotArchiveInfoGetter,
        },
        snapshot_bank_utils,
        snapshot_config::SnapshotConfig,
        snapshot_hash::{FullSnapshotHash, IncrementalSnapshotHash, StartingSnapshotHashes},
        snapshot_utils,
    },
    solana_sdk::genesis_config::GenesisConfig,
    std::{
        path::PathBuf,
        process, result,
        sync::{atomic::AtomicBool, Arc, RwLock},
    },
};

pub type LoadResult = result::Result<
    (
        Arc<RwLock<BankForks>>,
        LeaderScheduleCache,
        Option<StartingSnapshotHashes>,
    ),
    BlockstoreProcessorError,
>;

/// Load the banks via genesis or a snapshot then processes all full blocks in blockstore
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
    entry_notification_sender: Option<&EntryNotifierSender>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: Arc<AtomicBool>,
) -> LoadResult {
    let (bank_forks, leader_schedule_cache, starting_snapshot_hashes, ..) = load_bank_forks(
        genesis_config,
        blockstore,
        account_paths,
        shrink_paths,
        snapshot_config,
        &process_options,
        cache_block_meta_sender,
        entry_notification_sender,
        accounts_update_notifier,
        exit,
    );

    blockstore_processor::process_blockstore_from_root(
        blockstore,
        &bank_forks,
        &leader_schedule_cache,
        &process_options,
        transaction_status_sender,
        cache_block_meta_sender,
        entry_notification_sender,
        &AbsRequestSender::default(),
    )
    .map(|_| (bank_forks, leader_schedule_cache, starting_snapshot_hashes))
}

#[allow(clippy::too_many_arguments)]
pub fn load_bank_forks(
    genesis_config: &GenesisConfig,
    blockstore: &Blockstore,
    account_paths: Vec<PathBuf>,
    shrink_paths: Option<Vec<PathBuf>>,
    snapshot_config: Option<&SnapshotConfig>,
    process_options: &ProcessOptions,
    cache_block_meta_sender: Option<&CacheBlockMetaSender>,
    entry_notification_sender: Option<&EntryNotifierSender>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: Arc<AtomicBool>,
) -> (
    Arc<RwLock<BankForks>>,
    LeaderScheduleCache,
    Option<StartingSnapshotHashes>,
) {
    fn get_snapshots_to_load(
        snapshot_config: Option<&SnapshotConfig>,
    ) -> Option<(
        FullSnapshotArchiveInfo,
        Option<IncrementalSnapshotArchiveInfo>,
    )> {
        let Some(snapshot_config) = snapshot_config else {
            info!("Snapshots disabled; will load from genesis");
            return None;
        };

        let Some(full_snapshot_archive_info) =
            snapshot_utils::get_highest_full_snapshot_archive_info(
                &snapshot_config.full_snapshot_archives_dir,
            )
        else {
            warn!(
                "No snapshot package found in directory: {}; will load from genesis",
                snapshot_config.full_snapshot_archives_dir.display()
            );
            return None;
        };

        let incremental_snapshot_archive_info =
            snapshot_utils::get_highest_incremental_snapshot_archive_info(
                &snapshot_config.incremental_snapshot_archives_dir,
                full_snapshot_archive_info.slot(),
            );

        Some((
            full_snapshot_archive_info,
            incremental_snapshot_archive_info,
        ))
    }

    let (bank_forks, starting_snapshot_hashes) =
        if let Some((full_snapshot_archive_info, incremental_snapshot_archive_info)) =
            get_snapshots_to_load(snapshot_config)
        {
            // SAFETY: Having snapshots to load ensures a snapshot config
            let snapshot_config = snapshot_config.unwrap();
            info!(
                "Initializing bank snapshots dir: {}",
                snapshot_config.bank_snapshots_dir.display()
            );
            std::fs::create_dir_all(&snapshot_config.bank_snapshots_dir)
                .expect("create bank snapshots dir");
            let (bank_forks, starting_snapshot_hashes) = bank_forks_from_snapshot(
                full_snapshot_archive_info,
                incremental_snapshot_archive_info,
                genesis_config,
                account_paths,
                shrink_paths,
                snapshot_config,
                process_options,
                accounts_update_notifier,
                exit,
            );
            (bank_forks, Some(starting_snapshot_hashes))
        } else {
            let maybe_filler_accounts = process_options
                .accounts_db_config
                .as_ref()
                .map(|config| config.filler_accounts_config.count > 0);

            if let Some(true) = maybe_filler_accounts {
                panic!("filler accounts specified, but not loading from snapshot");
            }

            info!("Processing ledger from genesis");
            let bank_forks = blockstore_processor::process_blockstore_for_bank_0(
                genesis_config,
                blockstore,
                account_paths,
                process_options,
                cache_block_meta_sender,
                entry_notification_sender,
                accounts_update_notifier,
                exit,
            );
            bank_forks
                .read()
                .unwrap()
                .root_bank()
                .set_startup_verification_complete();

            (bank_forks, None)
        };

    let mut leader_schedule_cache =
        LeaderScheduleCache::new_from_bank(&bank_forks.read().unwrap().root_bank());
    if process_options.full_leader_cache {
        leader_schedule_cache.set_max_schedules(std::usize::MAX);
    }

    if let Some(ref new_hard_forks) = process_options.new_hard_forks {
        let root_bank = bank_forks.read().unwrap().root_bank();
        new_hard_forks
            .iter()
            .for_each(|hard_fork_slot| root_bank.register_hard_fork(*hard_fork_slot));
    }

    (bank_forks, leader_schedule_cache, starting_snapshot_hashes)
}

#[allow(clippy::too_many_arguments)]
fn bank_forks_from_snapshot(
    full_snapshot_archive_info: FullSnapshotArchiveInfo,
    incremental_snapshot_archive_info: Option<IncrementalSnapshotArchiveInfo>,
    genesis_config: &GenesisConfig,
    account_paths: Vec<PathBuf>,
    shrink_paths: Option<Vec<PathBuf>>,
    snapshot_config: &SnapshotConfig,
    process_options: &ProcessOptions,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: Arc<AtomicBool>,
) -> (Arc<RwLock<BankForks>>, StartingSnapshotHashes) {
    // Fail hard here if snapshot fails to load, don't silently continue
    if account_paths.is_empty() {
        error!("Account paths not present when booting from snapshot");
        process::exit(1);
    }

    let latest_snapshot_archive_slot = std::cmp::max(
        full_snapshot_archive_info.slot(),
        incremental_snapshot_archive_info
            .as_ref()
            .map(SnapshotArchiveInfoGetter::slot)
            .unwrap_or(0),
    );
    let latest_bank_snapshot =
        snapshot_utils::get_highest_bank_snapshot_post(&snapshot_config.bank_snapshots_dir);

    let will_startup_from_snapshot_archives = match process_options.use_snapshot_archives_at_startup
    {
        UseSnapshotArchivesAtStartup::Always => true,
        UseSnapshotArchivesAtStartup::Never => false,
        UseSnapshotArchivesAtStartup::WhenNewest => latest_bank_snapshot
            .as_ref()
            .map(|bank_snapshot| latest_snapshot_archive_slot > bank_snapshot.slot)
            .unwrap_or(true),
    };

    let bank = if will_startup_from_snapshot_archives {
        // Given that we are going to boot from an archive, the append vecs held in the snapshot dirs for fast-boot should
        // be released.  They will be released by the account_background_service anyway.  But in the case of the account_paths
        // using memory-mounted file system, they are not released early enough to give space for the new append-vecs from
        // the archives, causing the out-of-memory problem.  So, purge the snapshot dirs upfront before loading from the archive.
        snapshot_utils::purge_old_bank_snapshots(&snapshot_config.bank_snapshots_dir, 0, None);

        let (bank, _) = snapshot_bank_utils::bank_from_snapshot_archives(
            &account_paths,
            &snapshot_config.bank_snapshots_dir,
            &full_snapshot_archive_info,
            incremental_snapshot_archive_info.as_ref(),
            genesis_config,
            &process_options.runtime_config,
            process_options.debug_keys.clone(),
            None,
            process_options.account_indexes.clone(),
            process_options.limit_load_slot_count_from_snapshot,
            process_options.shrink_ratio,
            process_options.accounts_db_test_hash_calculation,
            process_options.accounts_db_skip_shrink,
            process_options.accounts_db_force_initial_clean,
            process_options.verify_index,
            process_options.accounts_db_config.clone(),
            accounts_update_notifier,
            exit,
        )
        .unwrap_or_else(|err| {
            error!(
                "Failed to load bank: {err} \
                \nfull snapshot archive: {} \
                \nincremental snapshot archive: {}",
                full_snapshot_archive_info.path().display(),
                incremental_snapshot_archive_info
                    .as_ref()
                    .map(|archive| archive.path().display().to_string())
                    .unwrap_or("none".to_string()),
            );
            process::exit(1);
        });
        bank
    } else {
        let Some(bank_snapshot) = latest_bank_snapshot else {
            error!(
                "There is no local state to startup from. Ensure --{} is *not* set to \"{}\" and restart.",
                use_snapshot_archives_at_startup::cli::LONG_ARG,
                UseSnapshotArchivesAtStartup::Never.to_string(),
            );
            process::exit(1);
        };

        // If a newer snapshot archive was downloaded, it is possible that its slot is
        // higher than the local bank we will load.  Did the user intend for this?
        if bank_snapshot.slot < latest_snapshot_archive_slot {
            assert_eq!(
                process_options.use_snapshot_archives_at_startup,
                UseSnapshotArchivesAtStartup::Never,
            );
            warn!(
                "Starting up from local state at slot {}, which is *older* than \
                the latest snapshot archive at slot {}. If this is not desired, \
                change the --{} CLI option to *not* \"{}\" and restart.",
                bank_snapshot.slot,
                latest_snapshot_archive_slot,
                use_snapshot_archives_at_startup::cli::LONG_ARG,
                UseSnapshotArchivesAtStartup::Never.to_string(),
            );
        }

        let (bank, _) = snapshot_bank_utils::bank_from_snapshot_dir(
            &account_paths,
            &bank_snapshot,
            genesis_config,
            &process_options.runtime_config,
            process_options.debug_keys.clone(),
            None,
            process_options.account_indexes.clone(),
            process_options.limit_load_slot_count_from_snapshot,
            process_options.shrink_ratio,
            process_options.verify_index,
            process_options.accounts_db_config.clone(),
            accounts_update_notifier,
            exit,
        )
        .unwrap_or_else(|err| {
            error!(
                "Failed to load bank: {err} \
                \nsnapshot: {}",
                bank_snapshot.snapshot_path().display(),
            );
            process::exit(1);
        });
        bank
    };

    if let Some(shrink_paths) = shrink_paths {
        bank.set_shrink_paths(shrink_paths);
    }

    let full_snapshot_hash = FullSnapshotHash((
        full_snapshot_archive_info.slot(),
        *full_snapshot_archive_info.hash(),
    ));
    let incremental_snapshot_hash =
        incremental_snapshot_archive_info.map(|incremental_snapshot_archive_info| {
            IncrementalSnapshotHash((
                incremental_snapshot_archive_info.slot(),
                *incremental_snapshot_archive_info.hash(),
            ))
        });
    let starting_snapshot_hashes = StartingSnapshotHashes {
        full: full_snapshot_hash,
        incremental: incremental_snapshot_hash,
    };

    (BankForks::new_rw_arc(bank), starting_snapshot_hashes)
}

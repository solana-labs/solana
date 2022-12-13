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
        accounts_background_service::AbsRequestSender,
        accounts_update_notifier_interface::AccountsUpdateNotifier,
        bank_forks::BankForks,
        snapshot_archive_info::SnapshotArchiveInfoGetter,
        snapshot_config::SnapshotConfig,
        snapshot_hash::{FullSnapshotHash, IncrementalSnapshotHash, StartingSnapshotHashes},
        snapshot_utils,
    },
    solana_sdk::genesis_config::GenesisConfig,
    std::{
        fs,
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
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: &Arc<AtomicBool>,
) -> LoadResult {
    let (bank_forks, leader_schedule_cache, starting_snapshot_hashes, ..) = load_bank_forks(
        genesis_config,
        blockstore,
        account_paths,
        shrink_paths,
        snapshot_config,
        &process_options,
        cache_block_meta_sender,
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
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: &Arc<AtomicBool>,
) -> (
    Arc<RwLock<BankForks>>,
    LeaderScheduleCache,
    Option<StartingSnapshotHashes>,
) {
    let snapshot_present = if let Some(snapshot_config) = snapshot_config {
        info!(
            "Initializing bank snapshot path: {}",
            snapshot_config.bank_snapshots_dir.display()
        );
        let _ = fs::remove_dir_all(&snapshot_config.bank_snapshots_dir);
        fs::create_dir_all(&snapshot_config.bank_snapshots_dir)
            .expect("Couldn't create snapshot directory");

        if snapshot_utils::get_highest_full_snapshot_archive_info(
            &snapshot_config.full_snapshot_archives_dir,
        )
        .is_some()
        {
            true
        } else {
            warn!(
                "No snapshot package found in directory: {:?}; will load from genesis",
                &snapshot_config.full_snapshot_archives_dir
            );
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
            process_options,
            accounts_update_notifier,
            exit,
        )
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
        let hard_forks = root_bank.hard_forks();

        for hard_fork_slot in new_hard_forks.iter() {
            if *hard_fork_slot > root_bank.slot() {
                hard_forks.write().unwrap().register(*hard_fork_slot);
            } else {
                warn!(
                    "Hard fork at {} ignored, --hard-fork option can be removed.",
                    hard_fork_slot
                );
            }
        }
    }

    (bank_forks, leader_schedule_cache, starting_snapshot_hashes)
}

#[allow(clippy::too_many_arguments)]
fn bank_forks_from_snapshot(
    genesis_config: &GenesisConfig,
    account_paths: Vec<PathBuf>,
    shrink_paths: Option<Vec<PathBuf>>,
    snapshot_config: &SnapshotConfig,
    process_options: &ProcessOptions,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: &Arc<AtomicBool>,
) -> (Arc<RwLock<BankForks>>, Option<StartingSnapshotHashes>) {
    // Fail hard here if snapshot fails to load, don't silently continue
    if account_paths.is_empty() {
        error!("Account paths not present when booting from snapshot");
        process::exit(1);
    }

    let (deserialized_bank, full_snapshot_archive_info, incremental_snapshot_archive_info) =
        snapshot_utils::bank_from_latest_snapshot_archives(
            &snapshot_config.bank_snapshots_dir,
            &snapshot_config.full_snapshot_archives_dir,
            &snapshot_config.incremental_snapshot_archives_dir,
            &account_paths,
            genesis_config,
            &process_options.runtime_config,
            process_options.debug_keys.clone(),
            Some(&crate::builtins::get(
                process_options.runtime_config.bpf_jit,
            )),
            process_options.account_indexes.clone(),
            process_options.limit_load_slot_count_from_snapshot,
            process_options.shrink_ratio,
            process_options.accounts_db_test_hash_calculation,
            process_options.accounts_db_skip_shrink,
            process_options.verify_index,
            process_options.accounts_db_config.clone(),
            accounts_update_notifier,
            exit,
        )
        .expect("Load from snapshot failed");

    if let Some(shrink_paths) = shrink_paths {
        deserialized_bank.set_shrink_paths(shrink_paths);
    }

    let full_snapshot_hash = FullSnapshotHash {
        hash: (
            full_snapshot_archive_info.slot(),
            full_snapshot_archive_info.hash().0,
        ),
    };
    let starting_incremental_snapshot_hash =
        incremental_snapshot_archive_info.map(|incremental_snapshot_archive_info| {
            IncrementalSnapshotHash {
                base: full_snapshot_hash.hash,
                hash: (
                    incremental_snapshot_archive_info.slot(),
                    incremental_snapshot_archive_info.hash().0,
                ),
            }
        });
    let starting_snapshot_hashes = StartingSnapshotHashes {
        full: full_snapshot_hash,
        incremental: starting_incremental_snapshot_hash,
    };

    (
        Arc::new(RwLock::new(BankForks::new(deserialized_bank))),
        Some(starting_snapshot_hashes),
    )
}

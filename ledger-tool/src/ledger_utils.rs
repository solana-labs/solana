use {
    crate::LEDGER_TOOL_DIRECTORY,
    clap::{value_t, value_t_or_exit, values_t_or_exit, ArgMatches},
    crossbeam_channel::unbounded,
    log::*,
    solana_accounts_db::{
        hardened_unpack::open_genesis_config,
        utils::{create_all_accounts_run_and_snapshot_dirs, move_and_async_delete_path_contents},
    },
    solana_core::{
        accounts_hash_verifier::AccountsHashVerifier, validator::BlockVerificationMethod,
    },
    solana_geyser_plugin_manager::geyser_plugin_service::{
        GeyserPluginService, GeyserPluginServiceError,
    },
    solana_ledger::{
        bank_forks_utils::{self, BankForksUtilsError},
        blockstore::{Blockstore, BlockstoreError},
        blockstore_options::{
            AccessType, BlockstoreOptions, BlockstoreRecoveryMode, LedgerColumnOptions,
            ShredStorageType,
        },
        blockstore_processor::{
            self, BlockstoreProcessorError, ProcessOptions, TransactionStatusSender,
        },
        use_snapshot_archives_at_startup::UseSnapshotArchivesAtStartup,
    },
    solana_measure::measure,
    solana_rpc::transaction_status_service::TransactionStatusService,
    solana_runtime::{
        accounts_background_service::{
            AbsRequestHandlers, AbsRequestSender, AccountsBackgroundService,
            PrunedBanksRequestHandler, SnapshotRequestHandler,
        },
        bank_forks::BankForks,
        prioritization_fee_cache::PrioritizationFeeCache,
        snapshot_config::SnapshotConfig,
        snapshot_hash::StartingSnapshotHashes,
        snapshot_utils::{self, clean_orphaned_account_snapshot_dirs},
    },
    solana_sdk::{
        clock::Slot, genesis_config::GenesisConfig, pubkey::Pubkey,
        transaction::VersionedTransaction,
    },
    solana_unified_scheduler_pool::DefaultSchedulerPool,
    std::{
        path::{Path, PathBuf},
        process::exit,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
    },
    thiserror::Error,
};

const PROCESS_SLOTS_HELP_STRING: &str =
    "The starting slot is either the latest found snapshot slot, or genesis (slot 0) if the \
     --no-snapshot flag was specified or if no snapshots were found. \
     The ending slot is the snapshot creation slot for create-snapshot, the value for \
     --halt-at-slot if specified, or the highest slot in the blockstore.";

#[derive(Error, Debug)]
pub(crate) enum LoadAndProcessLedgerError {
    #[error("failed to clean orphaned account snapshot directories: {0}")]
    CleanOrphanedAccountSnapshotDirectories(#[source] std::io::Error),

    #[error("failed to create all run and snapshot directories: {0}")]
    CreateAllAccountsRunAndSnapshotDirectories(#[source] std::io::Error),

    #[error("custom accounts path is not supported with seconday blockstore access")]
    CustomAccountsPathUnsupported(#[source] BlockstoreError),

    #[error(
        "failed to process blockstore from starting slot {0} to ending slot {1}; the ending slot \
         is less than the starting slot. {2}"
    )]
    EndingSlotLessThanStartingSlot(Slot, Slot, String),

    #[error(
        "failed to process blockstore from starting slot {0} to ending slot {1}; the blockstore \
         does not contain a replayable sequence of blocks between these slots. {2}"
    )]
    EndingSlotNotReachableFromStartingSlot(Slot, Slot, String),

    #[error("failed to setup geyser service: {0}")]
    GeyserServiceSetup(#[source] GeyserPluginServiceError),

    #[error("failed to load bank forks: {0}")]
    LoadBankForks(#[source] BankForksUtilsError),

    #[error("failed to process blockstore from root: {0}")]
    ProcessBlockstoreFromRoot(#[source] BlockstoreProcessorError),
}

pub fn load_and_process_ledger_or_exit(
    arg_matches: &ArgMatches,
    genesis_config: &GenesisConfig,
    blockstore: Arc<Blockstore>,
    process_options: ProcessOptions,
    snapshot_archive_path: Option<PathBuf>,
    incremental_snapshot_archive_path: Option<PathBuf>,
) -> (Arc<RwLock<BankForks>>, Option<StartingSnapshotHashes>) {
    load_and_process_ledger(
        arg_matches,
        genesis_config,
        blockstore,
        process_options,
        snapshot_archive_path,
        incremental_snapshot_archive_path,
    )
    .unwrap_or_else(|err| {
        eprintln!("Exiting. Failed to load and process ledger: {err}");
        exit(1);
    })
}

pub fn load_and_process_ledger(
    arg_matches: &ArgMatches,
    genesis_config: &GenesisConfig,
    blockstore: Arc<Blockstore>,
    process_options: ProcessOptions,
    snapshot_archive_path: Option<PathBuf>,
    incremental_snapshot_archive_path: Option<PathBuf>,
) -> Result<(Arc<RwLock<BankForks>>, Option<StartingSnapshotHashes>), LoadAndProcessLedgerError> {
    let bank_snapshots_dir = if blockstore.is_primary_access() {
        blockstore.ledger_path().join("snapshot")
    } else {
        blockstore
            .ledger_path()
            .join(LEDGER_TOOL_DIRECTORY)
            .join("snapshot")
    };

    let mut starting_slot = 0; // default start check with genesis
    let snapshot_config = if arg_matches.is_present("no_snapshot") {
        None
    } else {
        let full_snapshot_archives_dir =
            snapshot_archive_path.unwrap_or_else(|| blockstore.ledger_path().to_path_buf());
        let incremental_snapshot_archives_dir =
            incremental_snapshot_archive_path.unwrap_or_else(|| full_snapshot_archives_dir.clone());
        if let Some(full_snapshot_slot) =
            snapshot_utils::get_highest_full_snapshot_archive_slot(&full_snapshot_archives_dir)
        {
            let incremental_snapshot_slot =
                snapshot_utils::get_highest_incremental_snapshot_archive_slot(
                    &incremental_snapshot_archives_dir,
                    full_snapshot_slot,
                )
                .unwrap_or_default();
            starting_slot = std::cmp::max(full_snapshot_slot, incremental_snapshot_slot);
        }

        Some(SnapshotConfig {
            full_snapshot_archives_dir,
            incremental_snapshot_archives_dir,
            bank_snapshots_dir: bank_snapshots_dir.clone(),
            ..SnapshotConfig::new_load_only()
        })
    };

    match process_options.halt_at_slot {
        // Skip the following checks for sentinel values of Some(0) and None.
        // For Some(0), no slots will be be replayed after starting_slot.
        // For None, all available children of starting_slot will be replayed.
        None | Some(0) => {}
        Some(halt_slot) => {
            if halt_slot < starting_slot {
                return Err(LoadAndProcessLedgerError::EndingSlotLessThanStartingSlot(
                    starting_slot,
                    halt_slot,
                    PROCESS_SLOTS_HELP_STRING.to_string(),
                ));
            }
            // Check if we have the slot data necessary to replay from starting_slot to >= halt_slot.
            if !blockstore.slot_range_connected(starting_slot, halt_slot) {
                return Err(
                    LoadAndProcessLedgerError::EndingSlotNotReachableFromStartingSlot(
                        starting_slot,
                        halt_slot,
                        PROCESS_SLOTS_HELP_STRING.to_string(),
                    ),
                );
            }
        }
    }

    let account_paths = if let Some(account_paths) = arg_matches.value_of("account_paths") {
        // If this blockstore access is Primary, no other process (solana-validator) can hold
        // Primary access. So, allow a custom accounts path without worry of wiping the accounts
        // of solana-validator.
        if !blockstore.is_primary_access() {
            // Attempt to open the Blockstore in Primary access; if successful, no other process
            // was holding Primary so allow things to proceed with custom accounts path. Release
            // the Primary access instead of holding it to give priority to solana-validator over
            // solana-ledger-tool should solana-validator start before we've finished.
            info!(
                "Checking if another process currently holding Primary access to {:?}",
                blockstore.ledger_path()
            );
            Blockstore::open_with_options(
                blockstore.ledger_path(),
                BlockstoreOptions {
                    access_type: AccessType::PrimaryForMaintenance,
                    ..BlockstoreOptions::default()
                },
            )
            // Couldn't get Primary access, error out to be defensive.
            .map_err(LoadAndProcessLedgerError::CustomAccountsPathUnsupported)?;
        }
        account_paths.split(',').map(PathBuf::from).collect()
    } else if blockstore.is_primary_access() {
        vec![blockstore.ledger_path().join("accounts")]
    } else {
        let non_primary_accounts_path = blockstore
            .ledger_path()
            .join(LEDGER_TOOL_DIRECTORY)
            .join("accounts");
        info!(
            "Default accounts path is switched aligning with Blockstore's secondary access: {:?}",
            non_primary_accounts_path
        );
        vec![non_primary_accounts_path]
    };

    let (account_run_paths, account_snapshot_paths) =
        create_all_accounts_run_and_snapshot_dirs(&account_paths)
            .map_err(LoadAndProcessLedgerError::CreateAllAccountsRunAndSnapshotDirectories)?;
    // From now on, use run/ paths in the same way as the previous account_paths.
    let account_paths = account_run_paths;

    let (_, measure_clean_account_paths) = measure!(
        account_paths.iter().for_each(|path| {
            if path.exists() {
                info!("Cleaning contents of account path: {}", path.display());
                move_and_async_delete_path_contents(path);
            }
        }),
        "Cleaning account paths"
    );
    info!("{measure_clean_account_paths}");

    snapshot_utils::purge_incomplete_bank_snapshots(&bank_snapshots_dir);

    info!("Cleaning contents of account snapshot paths: {account_snapshot_paths:?}");
    clean_orphaned_account_snapshot_dirs(&bank_snapshots_dir, &account_snapshot_paths)
        .map_err(LoadAndProcessLedgerError::CleanOrphanedAccountSnapshotDirectories)?;

    let geyser_plugin_active = arg_matches.is_present("geyser_plugin_config");
    let (accounts_update_notifier, transaction_notifier) = if geyser_plugin_active {
        let geyser_config_files = values_t_or_exit!(arg_matches, "geyser_plugin_config", String)
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();

        let (confirmed_bank_sender, confirmed_bank_receiver) = unbounded();
        drop(confirmed_bank_sender);
        let geyser_service =
            GeyserPluginService::new(confirmed_bank_receiver, &geyser_config_files)
                .map_err(LoadAndProcessLedgerError::GeyserServiceSetup)?;
        (
            geyser_service.get_accounts_update_notifier(),
            geyser_service.get_transaction_notifier(),
        )
    } else {
        (None, None)
    };

    let exit = Arc::new(AtomicBool::new(false));
    let (bank_forks, leader_schedule_cache, starting_snapshot_hashes, ..) =
        bank_forks_utils::load_bank_forks(
            genesis_config,
            blockstore.as_ref(),
            account_paths,
            snapshot_config.as_ref(),
            &process_options,
            None,
            None, // Maybe support this later, though
            accounts_update_notifier,
            exit.clone(),
        )
        .map_err(LoadAndProcessLedgerError::LoadBankForks)?;
    let block_verification_method = value_t!(
        arg_matches,
        "block_verification_method",
        BlockVerificationMethod
    )
    .unwrap_or_default();
    info!(
        "Using: block-verification-method: {}",
        block_verification_method,
    );
    let unified_scheduler_handler_threads =
        value_t!(arg_matches, "unified_scheduler_handler_threads", usize).ok();
    match block_verification_method {
        BlockVerificationMethod::BlockstoreProcessor => {
            info!("no scheduler pool is installed for block verification...");
            if let Some(count) = unified_scheduler_handler_threads {
                warn!(
                    "--unified-scheduler-handler-threads={count} is ignored because unified \
                     scheduler isn't enabled"
                );
            }
        }
        BlockVerificationMethod::UnifiedScheduler => {
            let no_transaction_status_sender = None;
            let no_replay_vote_sender = None;
            let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
            bank_forks
                .write()
                .unwrap()
                .install_scheduler_pool(DefaultSchedulerPool::new_dyn(
                    unified_scheduler_handler_threads,
                    process_options.runtime_config.log_messages_bytes_limit,
                    no_transaction_status_sender,
                    no_replay_vote_sender,
                    ignored_prioritization_fee_cache,
                ));
        }
    }

    let (accounts_package_sender, accounts_package_receiver) = crossbeam_channel::unbounded();
    let accounts_hash_verifier = AccountsHashVerifier::new(
        accounts_package_sender.clone(),
        accounts_package_receiver,
        None,
        exit.clone(),
        SnapshotConfig::new_load_only(),
    );
    let (snapshot_request_sender, snapshot_request_receiver) = crossbeam_channel::unbounded();
    let accounts_background_request_sender = AbsRequestSender::new(snapshot_request_sender.clone());
    let snapshot_request_handler = SnapshotRequestHandler {
        snapshot_config: SnapshotConfig::new_load_only(),
        snapshot_request_sender,
        snapshot_request_receiver,
        accounts_package_sender,
    };
    let pruned_banks_receiver =
        AccountsBackgroundService::setup_bank_drop_callback(bank_forks.clone());
    let pruned_banks_request_handler = PrunedBanksRequestHandler {
        pruned_banks_receiver,
    };
    let abs_request_handler = AbsRequestHandlers {
        snapshot_request_handler,
        pruned_banks_request_handler,
    };
    let accounts_background_service = AccountsBackgroundService::new(
        bank_forks.clone(),
        exit.clone(),
        abs_request_handler,
        process_options.accounts_db_test_hash_calculation,
        None,
    );

    let enable_rpc_transaction_history = arg_matches.is_present("enable_rpc_transaction_history");

    let (transaction_status_sender, transaction_status_service) = if geyser_plugin_active
        || enable_rpc_transaction_history
    {
        // Need Primary (R/W) access to insert transaction data;
        // obtain Primary access if we do not already have it
        let tss_blockstore = if enable_rpc_transaction_history && !blockstore.is_primary_access() {
            Arc::new(open_blockstore(
                blockstore.ledger_path(),
                arg_matches,
                AccessType::PrimaryForMaintenance,
            ))
        } else {
            blockstore.clone()
        };

        let (transaction_status_sender, transaction_status_receiver) = unbounded();
        let transaction_status_service = TransactionStatusService::new(
            transaction_status_receiver,
            Arc::default(),
            enable_rpc_transaction_history,
            transaction_notifier,
            tss_blockstore,
            false,
            exit.clone(),
        );
        (
            Some(TransactionStatusSender {
                sender: transaction_status_sender,
            }),
            Some(transaction_status_service),
        )
    } else {
        (None, None)
    };

    let result = blockstore_processor::process_blockstore_from_root(
        blockstore.as_ref(),
        &bank_forks,
        &leader_schedule_cache,
        &process_options,
        transaction_status_sender.as_ref(),
        None,
        None, // Maybe support this later, though
        &accounts_background_request_sender,
    )
    .map(|_| (bank_forks, starting_snapshot_hashes))
    .map_err(LoadAndProcessLedgerError::ProcessBlockstoreFromRoot);

    exit.store(true, Ordering::Relaxed);
    accounts_background_service.join().unwrap();
    accounts_hash_verifier.join().unwrap();
    if let Some(service) = transaction_status_service {
        service.join().unwrap();
    }

    result
}

pub fn open_blockstore(
    ledger_path: &Path,
    matches: &ArgMatches,
    access_type: AccessType,
) -> Blockstore {
    let wal_recovery_mode = matches
        .value_of("wal_recovery_mode")
        .map(BlockstoreRecoveryMode::from);
    let force_update_to_open = matches.is_present("force_update_to_open");
    let enforce_ulimit_nofile = !matches.is_present("ignore_ulimit_nofile_error");
    let shred_storage_type = get_shred_storage_type(
        ledger_path,
        &format!(
            "Shred storage type cannot be inferred for ledger at {ledger_path:?}, using default \
             RocksLevel",
        ),
    );

    match Blockstore::open_with_options(
        ledger_path,
        BlockstoreOptions {
            access_type: access_type.clone(),
            recovery_mode: wal_recovery_mode.clone(),
            enforce_ulimit_nofile,
            column_options: LedgerColumnOptions {
                shred_storage_type,
                ..LedgerColumnOptions::default()
            },
        },
    ) {
        Ok(blockstore) => blockstore,
        Err(BlockstoreError::RocksDb(err)) => {
            // Missing essential file, indicative of blockstore not existing
            let missing_blockstore = err
                .to_string()
                .starts_with("IO error: No such file or directory:");
            // Missing column in blockstore that is expected by software
            let missing_column = err
                .to_string()
                .starts_with("Invalid argument: Column family not found:");
            // The blockstore settings with Primary access can resolve the
            // above issues automatically, so only emit the help messages
            // if access type is Secondary
            let is_secondary = access_type == AccessType::Secondary;

            if missing_blockstore && is_secondary {
                eprintln!(
                    "Failed to open blockstore at {ledger_path:?}, it is missing at least one \
                     critical file: {err:?}"
                );
            } else if missing_column && is_secondary {
                eprintln!(
                    "Failed to open blockstore at {ledger_path:?}, it does not have all necessary \
                     columns: {err:?}"
                );
            } else {
                eprintln!("Failed to open blockstore at {ledger_path:?}: {err:?}");
                exit(1);
            }
            if !force_update_to_open {
                eprintln!("Use --force-update-to-open flag to attempt to update the blockstore");
                exit(1);
            }
            open_blockstore_with_temporary_primary_access(
                ledger_path,
                access_type,
                wal_recovery_mode,
            )
            .unwrap_or_else(|err| {
                eprintln!(
                    "Failed to open blockstore (with --force-update-to-open) at {:?}: {:?}",
                    ledger_path, err
                );
                exit(1);
            })
        }
        Err(err) => {
            eprintln!("Failed to open blockstore at {ledger_path:?}: {err:?}");
            exit(1);
        }
    }
}

pub fn get_shred_storage_type(ledger_path: &Path, message: &str) -> ShredStorageType {
    // TODO: the following shred_storage_type inference must be updated once
    // the rocksdb options can be constructed via load_options_file() as the
    // value picked by passing None for `max_shred_storage_size` could affect
    // the persisted rocksdb options file.
    match ShredStorageType::from_ledger_path(ledger_path, None) {
        Some(s) => s,
        None => {
            info!("{}", message);
            ShredStorageType::RocksLevel
        }
    }
}

/// Open blockstore with temporary primary access to allow necessary,
/// persistent changes to be made to the blockstore (such as creation of new
/// column family(s)). Then, continue opening with `original_access_type`
fn open_blockstore_with_temporary_primary_access(
    ledger_path: &Path,
    original_access_type: AccessType,
    wal_recovery_mode: Option<BlockstoreRecoveryMode>,
) -> Result<Blockstore, BlockstoreError> {
    // Open with Primary will allow any configuration that automatically
    // updates to take effect
    info!("Attempting to temporarily open blockstore with Primary access in order to update");
    {
        let _ = Blockstore::open_with_options(
            ledger_path,
            BlockstoreOptions {
                access_type: AccessType::PrimaryForMaintenance,
                recovery_mode: wal_recovery_mode.clone(),
                enforce_ulimit_nofile: true,
                ..BlockstoreOptions::default()
            },
        )?;
    }
    // Now, attempt to open the blockstore with original AccessType
    info!(
        "Blockstore forced open succeeded, retrying with original access: {:?}",
        original_access_type
    );
    Blockstore::open_with_options(
        ledger_path,
        BlockstoreOptions {
            access_type: original_access_type,
            recovery_mode: wal_recovery_mode,
            enforce_ulimit_nofile: true,
            ..BlockstoreOptions::default()
        },
    )
}

pub fn open_genesis_config_by(ledger_path: &Path, matches: &ArgMatches<'_>) -> GenesisConfig {
    let max_genesis_archive_unpacked_size =
        value_t_or_exit!(matches, "max_genesis_archive_unpacked_size", u64);

    open_genesis_config(ledger_path, max_genesis_archive_unpacked_size).unwrap_or_else(|err| {
        eprintln!("Exiting. Failed to open genesis config: {err}");
        exit(1);
    })
}

pub fn get_program_ids(tx: &VersionedTransaction) -> impl Iterator<Item = &Pubkey> + '_ {
    let message = &tx.message;
    let account_keys = message.static_account_keys();

    message
        .instructions()
        .iter()
        .map(|ix| ix.program_id(account_keys))
}

/// Get the AccessType required, based on `process_options`
pub(crate) fn get_access_type(process_options: &ProcessOptions) -> AccessType {
    match process_options.use_snapshot_archives_at_startup {
        UseSnapshotArchivesAtStartup::Always => AccessType::Secondary,
        UseSnapshotArchivesAtStartup::Never => AccessType::PrimaryForMaintenance,
        UseSnapshotArchivesAtStartup::WhenNewest => AccessType::PrimaryForMaintenance,
    }
}

use {
    crate::LEDGER_TOOL_DIRECTORY,
    clap::{value_t, value_t_or_exit, values_t_or_exit, ArgMatches},
    crossbeam_channel::unbounded,
    log::*,
    solana_accounts_db::hardened_unpack::open_genesis_config,
    solana_core::{
        accounts_hash_verifier::AccountsHashVerifier, validator::BlockVerificationMethod,
    },
    solana_geyser_plugin_manager::geyser_plugin_service::GeyserPluginService,
    solana_gossip::{cluster_info::ClusterInfo, contact_info::ContactInfo},
    solana_ledger::{
        bank_forks_utils,
        blockstore::{Blockstore, BlockstoreError},
        blockstore_options::{
            AccessType, BlockstoreOptions, BlockstoreRecoveryMode, LedgerColumnOptions,
            ShredStorageType,
        },
        blockstore_processor::{
            self, BlockstoreProcessorError, ProcessOptions, TransactionStatusSender,
        },
    },
    solana_measure::measure,
    solana_rpc::transaction_status_service::TransactionStatusService,
    solana_runtime::{
        accounts_background_service::{
            AbsRequestHandlers, AbsRequestSender, AccountsBackgroundService,
            PrunedBanksRequestHandler, SnapshotRequestHandler,
        },
        bank_forks::BankForks,
        snapshot_config::SnapshotConfig,
        snapshot_hash::StartingSnapshotHashes,
        snapshot_utils::{
            self, clean_orphaned_account_snapshot_dirs, create_all_accounts_run_and_snapshot_dirs,
            move_and_async_delete_path_contents,
        },
    },
    solana_sdk::{
        genesis_config::GenesisConfig, signature::Signer, signer::keypair::Keypair,
        timing::timestamp,
    },
    solana_streamer::socket::SocketAddrSpace,
    std::{
        path::{Path, PathBuf},
        process::exit,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
    },
};

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

pub fn load_and_process_ledger(
    arg_matches: &ArgMatches,
    genesis_config: &GenesisConfig,
    blockstore: Arc<Blockstore>,
    process_options: ProcessOptions,
    snapshot_archive_path: Option<PathBuf>,
    incremental_snapshot_archive_path: Option<PathBuf>,
) -> Result<(Arc<RwLock<BankForks>>, Option<StartingSnapshotHashes>), BlockstoreProcessorError> {
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

    let start_slot_msg = "The starting slot will be the latest snapshot slot, or genesis if \
        the --no-snapshot flag is specified or if no snapshots are found.";
    match process_options.halt_at_slot {
        // Skip the following checks for sentinel values of Some(0) and None.
        // For Some(0), no slots will be be replayed after starting_slot.
        // For None, all available children of starting_slot will be replayed.
        None | Some(0) => {}
        Some(halt_slot) => {
            if halt_slot < starting_slot {
                eprintln!(
                    "Unable to process blockstore from starting slot {starting_slot} to \
                    {halt_slot}; the ending slot is less than the starting slot. {start_slot_msg}"
                );
                exit(1);
            }
            // Check if we have the slot data necessary to replay from starting_slot to >= halt_slot.
            if !blockstore.slot_range_connected(starting_slot, halt_slot) {
                eprintln!(
                    "Unable to process blockstore from starting slot {starting_slot} to \
                    {halt_slot}; the blockstore does not contain a replayable chain between these \
                    slots. {start_slot_msg}"
                );
                exit(1);
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
            if Blockstore::open_with_options(
                blockstore.ledger_path(),
                BlockstoreOptions {
                    access_type: AccessType::PrimaryForMaintenance,
                    ..BlockstoreOptions::default()
                },
            )
            .is_err()
            {
                // Couldn't get Primary access, error out to be defensive.
                eprintln!("Error: custom accounts path is not supported under secondary access");
                exit(1);
            }
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
        create_all_accounts_run_and_snapshot_dirs(&account_paths).unwrap_or_else(|err| {
            eprintln!("Error: {err}");
            exit(1);
        });

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
    if let Err(err) =
        clean_orphaned_account_snapshot_dirs(&bank_snapshots_dir, &account_snapshot_paths)
    {
        eprintln!("Failed to clean orphaned account snapshot dirs: {err}");
        exit(1);
    }

    let geyser_plugin_active = arg_matches.is_present("geyser_plugin_config");
    let (accounts_update_notifier, transaction_notifier) = if geyser_plugin_active {
        let geyser_config_files = values_t_or_exit!(arg_matches, "geyser_plugin_config", String)
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();

        let (confirmed_bank_sender, confirmed_bank_receiver) = unbounded();
        drop(confirmed_bank_sender);
        let geyser_service =
            GeyserPluginService::new(confirmed_bank_receiver, &geyser_config_files).unwrap_or_else(
                |err| {
                    eprintln!("Failed to setup Geyser service: {err}");
                    exit(1);
                },
            );
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
            None,
            snapshot_config.as_ref(),
            &process_options,
            None,
            None, // Maybe support this later, though
            accounts_update_notifier,
            exit.clone(),
        );
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

    let node_id = Arc::new(Keypair::new());
    let cluster_info = Arc::new(ClusterInfo::new(
        ContactInfo::new_localhost(&node_id.pubkey(), timestamp()),
        Arc::clone(&node_id),
        SocketAddrSpace::Unspecified,
    ));
    let (accounts_package_sender, accounts_package_receiver) = crossbeam_channel::unbounded();
    let accounts_hash_verifier = AccountsHashVerifier::new(
        accounts_package_sender.clone(),
        accounts_package_receiver,
        None,
        exit.clone(),
        cluster_info,
        None,
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

    let (transaction_status_sender, transaction_status_service) =
        if geyser_plugin_active || enable_rpc_transaction_history {
            // Need Primary (R/W) access to insert transaction data
            let tss_blockstore = if enable_rpc_transaction_history {
                Arc::new(open_blockstore(
                    blockstore.ledger_path(),
                    AccessType::PrimaryForMaintenance,
                    None,
                    false,
                    false,
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
    .map(|_| (bank_forks, starting_snapshot_hashes));

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
    access_type: AccessType,
    wal_recovery_mode: Option<BlockstoreRecoveryMode>,
    force_update_to_open: bool,
    enforce_ulimit_nofile: bool,
) -> Blockstore {
    let shred_storage_type = get_shred_storage_type(
        ledger_path,
        &format!(
            "Shred storage type cannot be inferred for ledger at {ledger_path:?}, \
         using default RocksLevel",
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
                    "Failed to open blockstore at {ledger_path:?}, it \
                    is missing at least one critical file: {err:?}"
                );
            } else if missing_column && is_secondary {
                eprintln!(
                    "Failed to open blockstore at {ledger_path:?}, it \
                    does not have all necessary columns: {err:?}"
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
    open_genesis_config(ledger_path, max_genesis_archive_unpacked_size)
}

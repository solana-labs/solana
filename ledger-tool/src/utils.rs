#![allow(clippy::integer_arithmetic)]
use {
    clap::{value_t_or_exit, values_t_or_exit, ArgMatches},
    crossbeam_channel::unbounded,
    log::*,
    solana_account_decoder::{UiAccount, UiAccountData, UiAccountEncoding},
    solana_geyser_plugin_manager::geyser_plugin_service::GeyserPluginService,
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
    solana_measure::measure::Measure,
    solana_rpc::{
        transaction_notifier_interface::TransactionNotifierLock,
        transaction_status_service::TransactionStatusService,
    },
    solana_runtime::{
        accounts_background_service::{
            AbsRequestHandlers, AbsRequestSender, AccountsBackgroundService,
            PrunedBanksRequestHandler, SnapshotRequestHandler,
        },
        accounts_update_notifier_interface::AccountsUpdateNotifier,
        bank_forks::BankForks,
        hardened_unpack::open_genesis_config,
        snapshot_config::SnapshotConfig,
        snapshot_hash::StartingSnapshotHashes,
        snapshot_utils::{self, create_accounts_run_and_snapshot_dirs, move_and_async_delete_path},
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Slot,
        genesis_config::GenesisConfig,
        native_token::lamports_to_sol,
        pubkey::Pubkey,
    },
    std::{
        path::{Path, PathBuf},
        process::exit,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
    },
};

pub(crate) fn open_genesis_config_by(
    ledger_path: &Path,
    matches: &ArgMatches<'_>,
) -> GenesisConfig {
    let max_genesis_archive_unpacked_size =
        value_t_or_exit!(matches, "max_genesis_archive_unpacked_size", u64);
    open_genesis_config(ledger_path, max_genesis_archive_unpacked_size)
}

// This function is duplicated in validator/src/main.rs...
pub(crate) fn hardforks_of(matches: &ArgMatches<'_>, name: &str) -> Option<Vec<Slot>> {
    if matches.is_present(name) {
        Some(values_t_or_exit!(matches, name, Slot))
    } else {
        None
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

pub(crate) fn open_blockstore(
    ledger_path: &Path,
    access_type: AccessType,
    wal_recovery_mode: Option<BlockstoreRecoveryMode>,
    force_update_to_open: bool,
) -> Blockstore {
    let shred_storage_type = get_shred_storage_type(
        ledger_path,
        &format!(
            "Shred stroage type cannot be inferred for ledger at {ledger_path:?}, \
         using default RocksLevel",
        ),
    );

    match Blockstore::open_with_options(
        ledger_path,
        BlockstoreOptions {
            access_type: access_type.clone(),
            recovery_mode: wal_recovery_mode.clone(),
            enforce_ulimit_nofile: true,
            column_options: LedgerColumnOptions {
                shred_storage_type,
                ..LedgerColumnOptions::default()
            },
        },
    ) {
        Ok(blockstore) => blockstore,
        Err(BlockstoreError::RocksDb(err))
            if (err
                .to_string()
                // Missing column family
                .starts_with("Invalid argument: Column family not found:")
                || err
                    .to_string()
                    // Missing essential file, indicative of blockstore not existing
                    .starts_with("IO error: No such file or directory:"))
                && access_type == AccessType::Secondary =>
        {
            error!("Blockstore is incompatible with current software and requires updates");
            if !force_update_to_open {
                error!("Use --force-update-to-open to allow blockstore to update");
                exit(1);
            }
            open_blockstore_with_temporary_primary_access(
                ledger_path,
                access_type,
                wal_recovery_mode,
            )
            .unwrap_or_else(|err| {
                error!(
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

pub(crate) fn load_bank_forks(
    arg_matches: &ArgMatches,
    genesis_config: &GenesisConfig,
    blockstore: Arc<Blockstore>,
    process_options: ProcessOptions,
    snapshot_archive_path: Option<PathBuf>,
    incremental_snapshot_archive_path: Option<PathBuf>,
) -> Result<(Arc<RwLock<BankForks>>, Option<StartingSnapshotHashes>), BlockstoreProcessorError> {
    let bank_snapshots_dir = blockstore
        .ledger_path()
        .join(if blockstore.is_primary_access() {
            "snapshot"
        } else {
            "snapshot.ledger-tool"
        });

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
            bank_snapshots_dir,
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
                eprintln!(
                "Unable to load bank forks at slot {halt_slot} because it is less than the starting slot {starting_slot}. \
                The starting slot will be the latest snapshot slot, or genesis if --no-snapshot flag specified or no snapshots found."
            );
                exit(1);
            }
            // Check if we have the slot data necessary to replay from starting_slot to >= halt_slot.
            if !blockstore.slot_range_connected(starting_slot, halt_slot) {
                eprintln!(
                    "Unable to load bank forks at slot {halt_slot} due to disconnected blocks.",
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
        let non_primary_accounts_path = blockstore.ledger_path().join("accounts.ledger-tool");
        info!(
            "Default accounts path is switched aligning with Blockstore's secondary access: {:?}",
            non_primary_accounts_path
        );
        vec![non_primary_accounts_path]
    };

    // For all account_paths, set up the run/ and snapshot/ sub directories.
    // If the sub directories do not exist, the account_path will be cleaned because older version put account files there
    let account_run_paths: Vec<PathBuf> = account_paths.into_iter().map(
        |account_path| {
            match create_accounts_run_and_snapshot_dirs(&account_path) {
                Ok((account_run_path, _account_snapshot_path)) => account_run_path,
                Err(err) => {
                    eprintln!("Unable to create account run and snapshot sub directories: {}, err: {err:?}", account_path.display());
                    exit(1);
                }
            }
        }).collect();

    // From now on, use run/ paths in the same way as the previous account_paths.
    let account_paths = account_run_paths;

    info!("Cleaning contents of account paths: {:?}", account_paths);
    let mut measure = Measure::start("clean_accounts_paths");
    account_paths.iter().for_each(|path| {
        if path.exists() {
            move_and_async_delete_path(path);
        }
    });
    measure.stop();
    info!("done. {}", measure);

    let mut accounts_update_notifier = Option::<AccountsUpdateNotifier>::default();
    let mut transaction_notifier = Option::<TransactionNotifierLock>::default();
    if arg_matches.is_present("geyser_plugin_config") {
        let geyser_config_files = values_t_or_exit!(arg_matches, "geyser_plugin_config", String)
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();

        let (confirmed_bank_sender, confirmed_bank_receiver) = unbounded();
        drop(confirmed_bank_sender);
        let geyser_service =
            GeyserPluginService::new(confirmed_bank_receiver, &geyser_config_files).unwrap_or_else(
                |err| {
                    eprintln!("Failed to setup Geyser service: {err:?}");
                    exit(1);
                },
            );
        accounts_update_notifier = geyser_service.get_accounts_update_notifier();
        transaction_notifier = geyser_service.get_transaction_notifier();
    }

    let (bank_forks, leader_schedule_cache, starting_snapshot_hashes, ..) =
        bank_forks_utils::load_bank_forks(
            genesis_config,
            blockstore.as_ref(),
            account_paths,
            None,
            snapshot_config.as_ref(),
            &process_options,
            None,
            accounts_update_notifier,
            &Arc::default(),
        );

    let (snapshot_request_sender, snapshot_request_receiver) = crossbeam_channel::unbounded();
    let (accounts_package_sender, _accounts_package_receiver) = crossbeam_channel::unbounded();
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
    let exit = Arc::new(AtomicBool::new(false));
    let accounts_background_service = AccountsBackgroundService::new(
        bank_forks.clone(),
        &exit,
        abs_request_handler,
        process_options.accounts_db_test_hash_calculation,
        None,
    );

    let (transaction_status_sender, transaction_status_service) = if transaction_notifier.is_some()
    {
        let (transaction_status_sender, transaction_status_receiver) = unbounded();
        let transaction_status_service = TransactionStatusService::new(
            transaction_status_receiver,
            Arc::default(),
            false,
            transaction_notifier,
            blockstore.clone(),
            false,
            &exit,
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
        &accounts_background_request_sender,
    )
    .map(|_| (bank_forks, starting_snapshot_hashes));

    exit.store(true, Ordering::Relaxed);
    accounts_background_service.join().unwrap();
    if let Some(service) = transaction_status_service {
        service.join().unwrap();
    }

    result
}

pub(crate) fn parse_encoding_format(matches: &ArgMatches<'_>) -> UiAccountEncoding {
    match matches.value_of("encoding") {
        Some("jsonParsed") => UiAccountEncoding::JsonParsed,
        Some("base64") => UiAccountEncoding::Base64,
        Some("base64+zstd") => UiAccountEncoding::Base64Zstd,
        _ => UiAccountEncoding::Base64,
    }
}

pub(crate) fn output_account(
    pubkey: &Pubkey,
    account: &AccountSharedData,
    modified_slot: Option<Slot>,
    print_account_data: bool,
    encoding: UiAccountEncoding,
) {
    println!("{pubkey}:");
    println!("  balance: {} SOL", lamports_to_sol(account.lamports()));
    println!("  owner: '{}'", account.owner());
    println!("  executable: {}", account.executable());
    if let Some(slot) = modified_slot {
        println!("  slot: {slot}");
    }
    println!("  rent_epoch: {}", account.rent_epoch());
    println!("  data_len: {}", account.data().len());
    if print_account_data {
        let account_data = UiAccount::encode(pubkey, account, encoding, None, None).data;
        match account_data {
            UiAccountData::Binary(data, data_encoding) => {
                println!("  data: '{data}'");
                println!(
                    "  encoding: {}",
                    serde_json::to_string(&data_encoding).unwrap()
                );
            }
            UiAccountData::Json(account_data) => {
                println!(
                    "  data: '{}'",
                    serde_json::to_string(&account_data).unwrap()
                );
                println!("  encoding: \"jsonParsed\"");
            }
            UiAccountData::LegacyBinary(_) => {}
        };
    }
}

pub(crate) fn get_shred_storage_type(ledger_path: &Path, message: &str) -> ShredStorageType {
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

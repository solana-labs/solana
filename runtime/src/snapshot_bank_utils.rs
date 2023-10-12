use {
    crate::{
        bank::{Bank, BankFieldsToDeserialize, BankSlotDelta},
        builtins::BuiltinPrototype,
        runtime_config::RuntimeConfig,
        serde_snapshot::{
            bank_from_streams, bank_to_stream, fields_from_streams,
            BankIncrementalSnapshotPersistence, SerdeStyle,
        },
        snapshot_archive_info::{
            FullSnapshotArchiveInfo, IncrementalSnapshotArchiveInfo, SnapshotArchiveInfoGetter,
        },
        snapshot_hash::SnapshotHash,
        snapshot_package::{AccountsPackage, AccountsPackageKind, SnapshotKind, SnapshotPackage},
        snapshot_utils::{
            self, archive_snapshot_package, build_storage_from_snapshot_dir,
            delete_contents_of_path, deserialize_snapshot_data_file,
            deserialize_snapshot_data_files, get_bank_snapshot_dir, get_highest_bank_snapshot_post,
            get_highest_full_snapshot_archive_info, get_highest_incremental_snapshot_archive_info,
            get_snapshot_file_name, get_storages_to_serialize, hard_link_storages_to_snapshot,
            serialize_snapshot_data_file, verify_and_unarchive_snapshots,
            verify_unpacked_snapshots_dir_and_version, write_snapshot_version_file,
            AddBankSnapshotError, ArchiveFormat, BankSnapshotInfo, BankSnapshotType, SnapshotError,
            SnapshotRootPaths, SnapshotVersion, StorageAndNextAppendVecId,
            UnpackedSnapshotsDirAndVersion, VerifySlotDeltasError,
        },
        status_cache,
    },
    bincode::{config::Options, serialize_into},
    fs_err,
    log::*,
    solana_accounts_db::{
        accounts_db::{
            AccountShrinkThreshold, AccountStorageEntry, AccountsDbConfig, AtomicAppendVecId,
            CalcAccountsHashDataSource,
        },
        accounts_hash::AccountsHash,
        accounts_index::AccountSecondaryIndexes,
        accounts_update_notifier_interface::AccountsUpdateNotifier,
    },
    solana_measure::{measure, measure::Measure},
    solana_sdk::{
        clock::Slot,
        feature_set,
        genesis_config::GenesisConfig,
        hash::Hash,
        pubkey::Pubkey,
        slot_history::{Check, SlotHistory},
    },
    std::{
        collections::HashSet,
        io::{BufWriter, Write},
        num::NonZeroUsize,
        path::{Path, PathBuf},
        sync::{atomic::AtomicBool, Arc},
    },
    tempfile::TempDir,
};

pub const DEFAULT_FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS: Slot = 25_000;
pub const DEFAULT_INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS: Slot = 100;
pub const DISABLED_SNAPSHOT_ARCHIVE_INTERVAL: Slot = Slot::MAX;

/// Serialize a bank to a snapshot
///
/// **DEVELOPER NOTE** Any error that is returned from this function may bring down the node!  This
/// function is called from AccountsBackgroundService to handle snapshot requests.  Since taking a
/// snapshot is not permitted to fail, any errors returned here will trigger the node to shutdown.
/// So, be careful whenever adding new code that may return errors.
pub fn add_bank_snapshot(
    bank_snapshots_dir: impl AsRef<Path>,
    bank: &Bank,
    snapshot_storages: &[Arc<AccountStorageEntry>],
    snapshot_version: SnapshotVersion,
    slot_deltas: Vec<BankSlotDelta>,
) -> snapshot_utils::Result<BankSnapshotInfo> {
    // this lambda function is to facilitate converting between
    // the AddBankSnapshotError and SnapshotError types
    let do_add_bank_snapshot = || {
        let mut measure_everything = Measure::start("");
        let slot = bank.slot();
        let bank_snapshot_dir = get_bank_snapshot_dir(&bank_snapshots_dir, slot);
        if bank_snapshot_dir.exists() {
            return Err(AddBankSnapshotError::SnapshotDirAlreadyExists(
                bank_snapshot_dir,
            ));
        }
        fs_err::create_dir_all(&bank_snapshot_dir)
            .map_err(AddBankSnapshotError::CreateSnapshotDir)?;

        // the bank snapshot is stored as bank_snapshots_dir/slot/slot.BANK_SNAPSHOT_PRE_FILENAME_EXTENSION
        let bank_snapshot_path = bank_snapshot_dir
            .join(get_snapshot_file_name(slot))
            .with_extension(snapshot_utils::BANK_SNAPSHOT_PRE_FILENAME_EXTENSION);

        info!(
            "Creating bank snapshot for slot {}, path: {}",
            slot,
            bank_snapshot_path.display(),
        );

        // We are constructing the snapshot directory to contain the full snapshot state information to allow
        // constructing a bank from this directory.  It acts like an archive to include the full state.
        // The set of the account storages files is the necessary part of this snapshot state.  Hard-link them
        // from the operational accounts/ directory to here.
        let (_, measure_hard_linking) =
            measure!(
                hard_link_storages_to_snapshot(&bank_snapshot_dir, slot, snapshot_storages)
                    .map_err(AddBankSnapshotError::HardLinkStorages)?
            );

        let bank_snapshot_serializer =
            move |stream: &mut BufWriter<std::fs::File>| -> snapshot_utils::Result<()> {
                let serde_style = match snapshot_version {
                    SnapshotVersion::V1_2_0 => SerdeStyle::Newer,
                };
                bank_to_stream(
                    serde_style,
                    stream.by_ref(),
                    bank,
                    &get_storages_to_serialize(snapshot_storages),
                )?;
                Ok(())
            };
        let (bank_snapshot_consumed_size, bank_serialize) = measure!(
            serialize_snapshot_data_file(&bank_snapshot_path, bank_snapshot_serializer)
                .map_err(|err| AddBankSnapshotError::SerializeBank(Box::new(err)))?,
            "bank serialize"
        );

        let status_cache_path =
            bank_snapshot_dir.join(snapshot_utils::SNAPSHOT_STATUS_CACHE_FILENAME);
        let (status_cache_consumed_size, status_cache_serialize) =
            measure!(serialize_status_cache(&slot_deltas, &status_cache_path)
                .map_err(|err| AddBankSnapshotError::SerializeStatusCache(Box::new(err)))?);

        let version_path = bank_snapshot_dir.join(snapshot_utils::SNAPSHOT_VERSION_FILENAME);
        let (_, measure_write_version_file) =
            measure!(write_snapshot_version_file(version_path, snapshot_version)
                .map_err(AddBankSnapshotError::WriteSnapshotVersionFile)?);

        // Mark this directory complete so it can be used.  Check this flag first before selecting for deserialization.
        let state_complete_path =
            bank_snapshot_dir.join(snapshot_utils::SNAPSHOT_STATE_COMPLETE_FILENAME);
        let (_, measure_write_state_complete_file) =
            measure!(fs_err::File::create(state_complete_path)
                .map_err(AddBankSnapshotError::CreateStateCompleteFile)?);

        measure_everything.stop();

        // Monitor sizes because they're capped to MAX_SNAPSHOT_DATA_FILE_SIZE
        datapoint_info!(
            "snapshot_bank",
            ("slot", slot, i64),
            ("bank_size", bank_snapshot_consumed_size, i64),
            ("status_cache_size", status_cache_consumed_size, i64),
            ("hard_link_storages_us", measure_hard_linking.as_us(), i64),
            ("bank_serialize_us", bank_serialize.as_us(), i64),
            (
                "status_cache_serialize_us",
                status_cache_serialize.as_us(),
                i64
            ),
            (
                "write_version_file_us",
                measure_write_version_file.as_us(),
                i64
            ),
            (
                "write_state_complete_file_us",
                measure_write_state_complete_file.as_us(),
                i64
            ),
            ("total_us", measure_everything.as_us(), i64),
        );

        info!(
            "{} for slot {} at {}",
            bank_serialize,
            slot,
            bank_snapshot_path.display(),
        );

        Ok(BankSnapshotInfo {
            slot,
            snapshot_type: BankSnapshotType::Pre,
            snapshot_dir: bank_snapshot_dir,
            snapshot_version,
        })
    };

    do_add_bank_snapshot().map_err(|err| SnapshotError::AddBankSnapshot(err, bank.slot()))
}

fn serialize_status_cache(
    slot_deltas: &[BankSlotDelta],
    status_cache_path: &Path,
) -> snapshot_utils::Result<u64> {
    serialize_snapshot_data_file(status_cache_path, |stream| {
        serialize_into(stream, slot_deltas)?;
        Ok(())
    })
}

#[derive(Debug, Default)]
pub struct BankFromArchiveTimings {
    pub rebuild_bank_from_snapshots_us: u64,
    pub full_snapshot_untar_us: u64,
    pub incremental_snapshot_untar_us: u64,
    pub verify_snapshot_bank_us: u64,
}

#[derive(Debug, Default)]
pub struct BankFromDirTimings {
    pub rebuild_bank_from_snapshot_us: u64,
    pub build_storage_us: u64,
}

/// Utility for parsing out bank specific information from a snapshot archive. This utility can be used
/// to parse out bank specific information like the leader schedule, epoch schedule, etc.
pub fn bank_fields_from_snapshot_archives(
    full_snapshot_archives_dir: impl AsRef<Path>,
    incremental_snapshot_archives_dir: impl AsRef<Path>,
) -> snapshot_utils::Result<BankFieldsToDeserialize> {
    let full_snapshot_archive_info =
        get_highest_full_snapshot_archive_info(&full_snapshot_archives_dir)
            .ok_or(SnapshotError::NoSnapshotArchives)?;

    let incremental_snapshot_archive_info = get_highest_incremental_snapshot_archive_info(
        &incremental_snapshot_archives_dir,
        full_snapshot_archive_info.slot(),
    );

    let temp_unpack_dir = TempDir::new()?;
    let temp_accounts_dir = TempDir::new()?;

    let account_paths = vec![temp_accounts_dir.path().to_path_buf()];

    let (unarchived_full_snapshot, unarchived_incremental_snapshot, _next_append_vec_id) =
        verify_and_unarchive_snapshots(
            &temp_unpack_dir,
            &full_snapshot_archive_info,
            incremental_snapshot_archive_info.as_ref(),
            &account_paths,
        )?;

    bank_fields_from_snapshots(
        &unarchived_full_snapshot.unpacked_snapshots_dir_and_version,
        unarchived_incremental_snapshot
            .as_ref()
            .map(|unarchive_preparation_result| {
                &unarchive_preparation_result.unpacked_snapshots_dir_and_version
            }),
    )
}

/// Rebuild bank from snapshot archives.  Handles either just a full snapshot, or both a full
/// snapshot and an incremental snapshot.
#[allow(clippy::too_many_arguments)]
pub fn bank_from_snapshot_archives(
    account_paths: &[PathBuf],
    bank_snapshots_dir: impl AsRef<Path>,
    full_snapshot_archive_info: &FullSnapshotArchiveInfo,
    incremental_snapshot_archive_info: Option<&IncrementalSnapshotArchiveInfo>,
    genesis_config: &GenesisConfig,
    runtime_config: &RuntimeConfig,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&[BuiltinPrototype]>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    test_hash_calculation: bool,
    accounts_db_skip_shrink: bool,
    accounts_db_force_initial_clean: bool,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: Arc<AtomicBool>,
) -> snapshot_utils::Result<(Bank, BankFromArchiveTimings)> {
    info!(
        "Loading bank from full snapshot archive: {}, and incremental snapshot archive: {:?}",
        full_snapshot_archive_info.path().display(),
        incremental_snapshot_archive_info
            .as_ref()
            .map(
                |incremental_snapshot_archive_info| incremental_snapshot_archive_info
                    .path()
                    .display()
            )
    );

    let (unarchived_full_snapshot, mut unarchived_incremental_snapshot, next_append_vec_id) =
        verify_and_unarchive_snapshots(
            bank_snapshots_dir,
            full_snapshot_archive_info,
            incremental_snapshot_archive_info,
            account_paths,
        )?;

    let mut storage = unarchived_full_snapshot.storage;
    if let Some(ref mut unarchive_preparation_result) = unarchived_incremental_snapshot {
        let incremental_snapshot_storages =
            std::mem::take(&mut unarchive_preparation_result.storage);
        storage.extend(incremental_snapshot_storages);
    }

    let storage_and_next_append_vec_id = StorageAndNextAppendVecId {
        storage,
        next_append_vec_id,
    };

    let mut measure_rebuild = Measure::start("rebuild bank from snapshots");
    let bank = rebuild_bank_from_unarchived_snapshots(
        &unarchived_full_snapshot.unpacked_snapshots_dir_and_version,
        unarchived_incremental_snapshot
            .as_ref()
            .map(|unarchive_preparation_result| {
                &unarchive_preparation_result.unpacked_snapshots_dir_and_version
            }),
        account_paths,
        storage_and_next_append_vec_id,
        genesis_config,
        runtime_config,
        debug_keys,
        additional_builtins,
        account_secondary_indexes,
        limit_load_slot_count_from_snapshot,
        shrink_ratio,
        verify_index,
        accounts_db_config,
        accounts_update_notifier,
        exit,
    )?;
    measure_rebuild.stop();
    info!("{}", measure_rebuild);

    let snapshot_archive_info = incremental_snapshot_archive_info.map_or_else(
        || full_snapshot_archive_info.snapshot_archive_info(),
        |incremental_snapshot_archive_info| {
            incremental_snapshot_archive_info.snapshot_archive_info()
        },
    );
    verify_bank_against_expected_slot_hash(
        &bank,
        snapshot_archive_info.slot,
        snapshot_archive_info.hash,
    )?;

    let base = (incremental_snapshot_archive_info.is_some()
        && bank
            .feature_set
            .is_active(&feature_set::incremental_snapshot_only_incremental_hash_calculation::id()))
    .then(|| {
        let base_slot = full_snapshot_archive_info.slot();
        let base_capitalization = bank
            .rc
            .accounts
            .accounts_db
            .get_accounts_hash(base_slot)
            .expect("accounts hash must exist at full snapshot's slot")
            .1;
        (base_slot, base_capitalization)
    });

    let mut measure_verify = Measure::start("verify");
    if !bank.verify_snapshot_bank(
        test_hash_calculation,
        accounts_db_skip_shrink || !full_snapshot_archive_info.is_remote(),
        accounts_db_force_initial_clean,
        full_snapshot_archive_info.slot(),
        base,
    ) && limit_load_slot_count_from_snapshot.is_none()
    {
        panic!("Snapshot bank for slot {} failed to verify", bank.slot());
    }
    measure_verify.stop();

    let timings = BankFromArchiveTimings {
        rebuild_bank_from_snapshots_us: measure_rebuild.as_us(),
        full_snapshot_untar_us: unarchived_full_snapshot.measure_untar.as_us(),
        incremental_snapshot_untar_us: unarchived_incremental_snapshot
            .map_or(0, |unarchive_preparation_result| {
                unarchive_preparation_result.measure_untar.as_us()
            }),
        verify_snapshot_bank_us: measure_verify.as_us(),
    };
    datapoint_info!(
        "bank_from_snapshot_archives",
        (
            "full_snapshot_untar_us",
            timings.full_snapshot_untar_us,
            i64
        ),
        (
            "incremental_snapshot_untar_us",
            timings.incremental_snapshot_untar_us,
            i64
        ),
        (
            "rebuild_bank_from_snapshots_us",
            timings.rebuild_bank_from_snapshots_us,
            i64
        ),
        (
            "verify_snapshot_bank_us",
            timings.verify_snapshot_bank_us,
            i64
        ),
    );
    Ok((bank, timings))
}

/// Rebuild bank from snapshot archives
///
/// This function searches `full_snapshot_archives_dir` and `incremental_snapshot_archives_dir` for
/// the highest full snapshot and highest corresponding incremental snapshot, then rebuilds the bank.
#[allow(clippy::too_many_arguments)]
pub fn bank_from_latest_snapshot_archives(
    bank_snapshots_dir: impl AsRef<Path>,
    full_snapshot_archives_dir: impl AsRef<Path>,
    incremental_snapshot_archives_dir: impl AsRef<Path>,
    account_paths: &[PathBuf],
    genesis_config: &GenesisConfig,
    runtime_config: &RuntimeConfig,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&[BuiltinPrototype]>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    test_hash_calculation: bool,
    accounts_db_skip_shrink: bool,
    accounts_db_force_initial_clean: bool,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: Arc<AtomicBool>,
) -> snapshot_utils::Result<(
    Bank,
    FullSnapshotArchiveInfo,
    Option<IncrementalSnapshotArchiveInfo>,
)> {
    let full_snapshot_archive_info =
        get_highest_full_snapshot_archive_info(&full_snapshot_archives_dir)
            .ok_or(SnapshotError::NoSnapshotArchives)?;

    let incremental_snapshot_archive_info = get_highest_incremental_snapshot_archive_info(
        &incremental_snapshot_archives_dir,
        full_snapshot_archive_info.slot(),
    );

    let (bank, _) = bank_from_snapshot_archives(
        account_paths,
        bank_snapshots_dir.as_ref(),
        &full_snapshot_archive_info,
        incremental_snapshot_archive_info.as_ref(),
        genesis_config,
        runtime_config,
        debug_keys,
        additional_builtins,
        account_secondary_indexes,
        limit_load_slot_count_from_snapshot,
        shrink_ratio,
        test_hash_calculation,
        accounts_db_skip_shrink,
        accounts_db_force_initial_clean,
        verify_index,
        accounts_db_config,
        accounts_update_notifier,
        exit,
    )?;

    Ok((
        bank,
        full_snapshot_archive_info,
        incremental_snapshot_archive_info,
    ))
}

/// Build bank from a snapshot (a snapshot directory, not a snapshot archive)
#[allow(clippy::too_many_arguments)]
pub fn bank_from_snapshot_dir(
    account_paths: &[PathBuf],
    bank_snapshot: &BankSnapshotInfo,
    genesis_config: &GenesisConfig,
    runtime_config: &RuntimeConfig,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&[BuiltinPrototype]>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: Arc<AtomicBool>,
) -> snapshot_utils::Result<(Bank, BankFromDirTimings)> {
    info!(
        "Loading bank from snapshot dir: {}",
        bank_snapshot.snapshot_dir.display()
    );

    // Clear the contents of the account paths run directories.  When constructing the bank, the appendvec
    // files will be extracted from the snapshot hardlink directories into these run/ directories.
    for path in account_paths {
        delete_contents_of_path(path);
    }

    let next_append_vec_id = Arc::new(AtomicAppendVecId::new(0));

    let (storage, measure_build_storage) = measure!(
        build_storage_from_snapshot_dir(bank_snapshot, account_paths, next_append_vec_id.clone())?,
        "build storage from snapshot dir"
    );
    info!("{}", measure_build_storage);

    let next_append_vec_id =
        Arc::try_unwrap(next_append_vec_id).expect("this is the only strong reference");
    let storage_and_next_append_vec_id = StorageAndNextAppendVecId {
        storage,
        next_append_vec_id,
    };
    let mut measure_rebuild = Measure::start("rebuild bank from snapshot");
    let bank = rebuild_bank_from_snapshot(
        bank_snapshot,
        account_paths,
        storage_and_next_append_vec_id,
        genesis_config,
        runtime_config,
        debug_keys,
        additional_builtins,
        account_secondary_indexes,
        limit_load_slot_count_from_snapshot,
        shrink_ratio,
        verify_index,
        accounts_db_config,
        accounts_update_notifier,
        exit,
    )?;
    measure_rebuild.stop();
    info!("{}", measure_rebuild);

    // Skip bank.verify_snapshot_bank.  Subsequent snapshot requests/accounts hash verification requests
    // will calculate and check the accounts hash, so we will still have safety/correctness there.
    bank.set_initial_accounts_hash_verification_completed();

    let timings = BankFromDirTimings {
        rebuild_bank_from_snapshot_us: measure_rebuild.as_us(),
        build_storage_us: measure_build_storage.as_us(),
    };
    datapoint_info!(
        "bank_from_snapshot_dir",
        (
            "build_storage_from_snapshot_dir_us",
            timings.build_storage_us,
            i64
        ),
        (
            "rebuild_bank_from_snapshot_us",
            timings.rebuild_bank_from_snapshot_us,
            i64
        ),
    );
    Ok((bank, timings))
}

/// follow the prototype of fn bank_from_latest_snapshot_archives, implement the from_dir case
#[allow(clippy::too_many_arguments)]
pub fn bank_from_latest_snapshot_dir(
    bank_snapshots_dir: impl AsRef<Path>,
    genesis_config: &GenesisConfig,
    runtime_config: &RuntimeConfig,
    account_paths: &[PathBuf],
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&[BuiltinPrototype]>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: Arc<AtomicBool>,
) -> snapshot_utils::Result<Bank> {
    let bank_snapshot = get_highest_bank_snapshot_post(&bank_snapshots_dir).ok_or_else(|| {
        SnapshotError::NoSnapshotSlotDir(bank_snapshots_dir.as_ref().to_path_buf())
    })?;

    let (bank, _) = bank_from_snapshot_dir(
        account_paths,
        &bank_snapshot,
        genesis_config,
        runtime_config,
        debug_keys,
        additional_builtins,
        account_secondary_indexes,
        limit_load_slot_count_from_snapshot,
        shrink_ratio,
        verify_index,
        accounts_db_config,
        accounts_update_notifier,
        exit,
    )?;

    Ok(bank)
}

/// Check to make sure the deserialized bank's slot and hash matches the snapshot archive's slot
/// and hash
fn verify_bank_against_expected_slot_hash(
    bank: &Bank,
    expected_slot: Slot,
    expected_hash: SnapshotHash,
) -> snapshot_utils::Result<()> {
    let bank_slot = bank.slot();
    let bank_hash = bank.get_snapshot_hash();

    if bank_slot != expected_slot || bank_hash != expected_hash {
        return Err(SnapshotError::MismatchedSlotHash(
            (bank_slot, bank_hash),
            (expected_slot, expected_hash),
        ));
    }

    Ok(())
}

fn bank_fields_from_snapshots(
    full_snapshot_unpacked_snapshots_dir_and_version: &UnpackedSnapshotsDirAndVersion,
    incremental_snapshot_unpacked_snapshots_dir_and_version: Option<
        &UnpackedSnapshotsDirAndVersion,
    >,
) -> snapshot_utils::Result<BankFieldsToDeserialize> {
    let (full_snapshot_version, full_snapshot_root_paths) =
        verify_unpacked_snapshots_dir_and_version(
            full_snapshot_unpacked_snapshots_dir_and_version,
        )?;
    let (incremental_snapshot_version, incremental_snapshot_root_paths) =
        if let Some(snapshot_unpacked_snapshots_dir_and_version) =
            incremental_snapshot_unpacked_snapshots_dir_and_version
        {
            let (snapshot_version, bank_snapshot_info) = verify_unpacked_snapshots_dir_and_version(
                snapshot_unpacked_snapshots_dir_and_version,
            )?;
            (Some(snapshot_version), Some(bank_snapshot_info))
        } else {
            (None, None)
        };
    info!(
        "Loading bank from full snapshot {} and incremental snapshot {:?}",
        full_snapshot_root_paths.snapshot_path().display(),
        incremental_snapshot_root_paths
            .as_ref()
            .map(|paths| paths.snapshot_path()),
    );

    let snapshot_root_paths = SnapshotRootPaths {
        full_snapshot_root_file_path: full_snapshot_root_paths.snapshot_path(),
        incremental_snapshot_root_file_path: incremental_snapshot_root_paths
            .map(|root_paths| root_paths.snapshot_path()),
    };

    deserialize_snapshot_data_files(&snapshot_root_paths, |snapshot_streams| {
        Ok(
            match incremental_snapshot_version.unwrap_or(full_snapshot_version) {
                SnapshotVersion::V1_2_0 => fields_from_streams(SerdeStyle::Newer, snapshot_streams)
                    .map(|(bank_fields, _accountsdb_fields)| bank_fields.collapse_into()),
            }?,
        )
    })
}

fn deserialize_status_cache(
    status_cache_path: &Path,
) -> snapshot_utils::Result<Vec<BankSlotDelta>> {
    deserialize_snapshot_data_file(status_cache_path, |stream| {
        info!(
            "Rebuilding status cache from {}",
            status_cache_path.display()
        );
        let slot_delta: Vec<BankSlotDelta> = bincode::options()
            .with_limit(snapshot_utils::MAX_SNAPSHOT_DATA_FILE_SIZE)
            .with_fixint_encoding()
            .allow_trailing_bytes()
            .deserialize_from(stream)?;
        Ok(slot_delta)
    })
}

#[allow(clippy::too_many_arguments)]
fn rebuild_bank_from_unarchived_snapshots(
    full_snapshot_unpacked_snapshots_dir_and_version: &UnpackedSnapshotsDirAndVersion,
    incremental_snapshot_unpacked_snapshots_dir_and_version: Option<
        &UnpackedSnapshotsDirAndVersion,
    >,
    account_paths: &[PathBuf],
    storage_and_next_append_vec_id: StorageAndNextAppendVecId,
    genesis_config: &GenesisConfig,
    runtime_config: &RuntimeConfig,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&[BuiltinPrototype]>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: Arc<AtomicBool>,
) -> snapshot_utils::Result<Bank> {
    let (full_snapshot_version, full_snapshot_root_paths) =
        verify_unpacked_snapshots_dir_and_version(
            full_snapshot_unpacked_snapshots_dir_and_version,
        )?;
    let (incremental_snapshot_version, incremental_snapshot_root_paths) =
        if let Some(snapshot_unpacked_snapshots_dir_and_version) =
            incremental_snapshot_unpacked_snapshots_dir_and_version
        {
            Some(verify_unpacked_snapshots_dir_and_version(
                snapshot_unpacked_snapshots_dir_and_version,
            )?)
        } else {
            None
        }
        .unzip();
    info!(
        "Rebuilding bank from full snapshot {} and incremental snapshot {:?}",
        full_snapshot_root_paths.snapshot_path().display(),
        incremental_snapshot_root_paths
            .as_ref()
            .map(|paths| paths.snapshot_path()),
    );

    let snapshot_root_paths = SnapshotRootPaths {
        full_snapshot_root_file_path: full_snapshot_root_paths.snapshot_path(),
        incremental_snapshot_root_file_path: incremental_snapshot_root_paths
            .map(|root_paths| root_paths.snapshot_path()),
    };

    let bank = deserialize_snapshot_data_files(&snapshot_root_paths, |snapshot_streams| {
        Ok(
            match incremental_snapshot_version.unwrap_or(full_snapshot_version) {
                SnapshotVersion::V1_2_0 => bank_from_streams(
                    SerdeStyle::Newer,
                    snapshot_streams,
                    account_paths,
                    storage_and_next_append_vec_id,
                    genesis_config,
                    runtime_config,
                    debug_keys,
                    additional_builtins,
                    account_secondary_indexes,
                    limit_load_slot_count_from_snapshot,
                    shrink_ratio,
                    verify_index,
                    accounts_db_config,
                    accounts_update_notifier,
                    exit,
                ),
            }?,
        )
    })?;

    // The status cache is rebuilt from the latest snapshot.  So, if there's an incremental
    // snapshot, use that.  Otherwise use the full snapshot.
    let status_cache_path = incremental_snapshot_unpacked_snapshots_dir_and_version
        .map_or_else(
            || {
                full_snapshot_unpacked_snapshots_dir_and_version
                    .unpacked_snapshots_dir
                    .as_path()
            },
            |unpacked_snapshots_dir_and_version| {
                unpacked_snapshots_dir_and_version
                    .unpacked_snapshots_dir
                    .as_path()
            },
        )
        .join(snapshot_utils::SNAPSHOT_STATUS_CACHE_FILENAME);
    let slot_deltas = deserialize_status_cache(&status_cache_path)?;

    verify_slot_deltas(slot_deltas.as_slice(), &bank)?;

    bank.status_cache.write().unwrap().append(&slot_deltas);

    info!("Rebuilt bank for slot: {}", bank.slot());
    Ok(bank)
}

#[allow(clippy::too_many_arguments)]
fn rebuild_bank_from_snapshot(
    bank_snapshot: &BankSnapshotInfo,
    account_paths: &[PathBuf],
    storage_and_next_append_vec_id: StorageAndNextAppendVecId,
    genesis_config: &GenesisConfig,
    runtime_config: &RuntimeConfig,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&[BuiltinPrototype]>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: Arc<AtomicBool>,
) -> snapshot_utils::Result<Bank> {
    info!(
        "Rebuilding bank from snapshot {}",
        bank_snapshot.snapshot_dir.display(),
    );

    let snapshot_root_paths = SnapshotRootPaths {
        full_snapshot_root_file_path: bank_snapshot.snapshot_path(),
        incremental_snapshot_root_file_path: None,
    };

    let bank = deserialize_snapshot_data_files(&snapshot_root_paths, |snapshot_streams| {
        Ok(bank_from_streams(
            SerdeStyle::Newer,
            snapshot_streams,
            account_paths,
            storage_and_next_append_vec_id,
            genesis_config,
            runtime_config,
            debug_keys,
            additional_builtins,
            account_secondary_indexes,
            limit_load_slot_count_from_snapshot,
            shrink_ratio,
            verify_index,
            accounts_db_config,
            accounts_update_notifier,
            exit,
        )?)
    })?;

    let status_cache_path = bank_snapshot
        .snapshot_dir
        .join(snapshot_utils::SNAPSHOT_STATUS_CACHE_FILENAME);
    let slot_deltas = deserialize_status_cache(&status_cache_path)?;

    verify_slot_deltas(slot_deltas.as_slice(), &bank)?;

    bank.status_cache.write().unwrap().append(&slot_deltas);

    info!("Rebuilt bank for slot: {}", bank.slot());
    Ok(bank)
}

/// Verify that the snapshot's slot deltas are not corrupt/invalid
fn verify_slot_deltas(
    slot_deltas: &[BankSlotDelta],
    bank: &Bank,
) -> std::result::Result<(), VerifySlotDeltasError> {
    let info = verify_slot_deltas_structural(slot_deltas, bank.slot())?;
    verify_slot_deltas_with_history(&info.slots, &bank.get_slot_history(), bank.slot())
}

/// Verify that the snapshot's slot deltas are not corrupt/invalid
/// These checks are simple/structural
fn verify_slot_deltas_structural(
    slot_deltas: &[BankSlotDelta],
    bank_slot: Slot,
) -> std::result::Result<VerifySlotDeltasStructuralInfo, VerifySlotDeltasError> {
    // there should not be more entries than that status cache's max
    let num_entries = slot_deltas.len();
    if num_entries > status_cache::MAX_CACHE_ENTRIES {
        return Err(VerifySlotDeltasError::TooManyEntries(
            num_entries,
            status_cache::MAX_CACHE_ENTRIES,
        ));
    }

    let mut slots_seen_so_far = HashSet::new();
    for &(slot, is_root, ..) in slot_deltas {
        // all entries should be roots
        if !is_root {
            return Err(VerifySlotDeltasError::SlotIsNotRoot(slot));
        }

        // all entries should be for slots less than or equal to the bank's slot
        if slot > bank_slot {
            return Err(VerifySlotDeltasError::SlotGreaterThanMaxRoot(
                slot, bank_slot,
            ));
        }

        // there should only be one entry per slot
        let is_duplicate = !slots_seen_so_far.insert(slot);
        if is_duplicate {
            return Err(VerifySlotDeltasError::SlotHasMultipleEntries(slot));
        }
    }

    // detect serious logic error for future careless changes. :)
    assert_eq!(slots_seen_so_far.len(), slot_deltas.len());

    Ok(VerifySlotDeltasStructuralInfo {
        slots: slots_seen_so_far,
    })
}

/// Computed information from `verify_slot_deltas_structural()`, that may be reused/useful later.
#[derive(Debug, PartialEq, Eq)]
struct VerifySlotDeltasStructuralInfo {
    /// All the slots in the slot deltas
    slots: HashSet<Slot>,
}

/// Verify that the snapshot's slot deltas are not corrupt/invalid
/// These checks use the slot history for verification
fn verify_slot_deltas_with_history(
    slots_from_slot_deltas: &HashSet<Slot>,
    slot_history: &SlotHistory,
    bank_slot: Slot,
) -> std::result::Result<(), VerifySlotDeltasError> {
    // ensure the slot history is valid (as much as possible), since we're using it to verify the
    // slot deltas
    if slot_history.newest() != bank_slot {
        return Err(VerifySlotDeltasError::BadSlotHistory);
    }

    // all slots in the slot deltas should be in the bank's slot history
    let slot_missing_from_history = slots_from_slot_deltas
        .iter()
        .find(|slot| slot_history.check(**slot) != Check::Found);
    if let Some(slot) = slot_missing_from_history {
        return Err(VerifySlotDeltasError::SlotNotFoundInHistory(*slot));
    }

    // all slots in the history should be in the slot deltas (up to MAX_CACHE_ENTRIES)
    // this ensures nothing was removed from the status cache
    //
    // go through the slot history and make sure there's an entry for each slot
    // note: it's important to go highest-to-lowest since the status cache removes
    // older entries first
    // note: we already checked above that `bank_slot == slot_history.newest()`
    let slot_missing_from_deltas = (slot_history.oldest()..=slot_history.newest())
        .rev()
        .filter(|slot| slot_history.check(*slot) == Check::Found)
        .take(status_cache::MAX_CACHE_ENTRIES)
        .find(|slot| !slots_from_slot_deltas.contains(slot));
    if let Some(slot) = slot_missing_from_deltas {
        return Err(VerifySlotDeltasError::SlotNotFoundInDeltas(slot));
    }

    Ok(())
}

/// Get the snapshot storages for this bank
pub fn get_snapshot_storages(bank: &Bank) -> Vec<Arc<AccountStorageEntry>> {
    let mut measure_snapshot_storages = Measure::start("snapshot-storages");
    let snapshot_storages = bank.get_snapshot_storages(None);
    measure_snapshot_storages.stop();
    datapoint_info!(
        "get_snapshot_storages",
        (
            "snapshot-storages-time-ms",
            measure_snapshot_storages.as_ms(),
            i64
        ),
    );

    snapshot_storages
}

/// Convenience function to create a full snapshot archive out of any Bank, regardless of state.
/// The Bank will be frozen during the process.
/// This is only called from ledger-tool or tests. Warping is a special case as well.
///
/// Requires:
///     - `bank` is complete
pub fn bank_to_full_snapshot_archive(
    bank_snapshots_dir: impl AsRef<Path>,
    bank: &Bank,
    snapshot_version: Option<SnapshotVersion>,
    full_snapshot_archives_dir: impl AsRef<Path>,
    incremental_snapshot_archives_dir: impl AsRef<Path>,
    archive_format: ArchiveFormat,
    maximum_full_snapshot_archives_to_retain: NonZeroUsize,
    maximum_incremental_snapshot_archives_to_retain: NonZeroUsize,
) -> snapshot_utils::Result<FullSnapshotArchiveInfo> {
    let snapshot_version = snapshot_version.unwrap_or_default();

    assert!(bank.is_complete());
    bank.squash(); // Bank may not be a root
    bank.force_flush_accounts_cache();
    bank.clean_accounts(Some(bank.slot()));
    bank.update_accounts_hash(CalcAccountsHashDataSource::Storages, false, false);
    bank.rehash(); // Bank accounts may have been manually modified by the caller

    let temp_dir = tempfile::tempdir_in(bank_snapshots_dir)?;
    let snapshot_storages = bank.get_snapshot_storages(None);
    let slot_deltas = bank.status_cache.read().unwrap().root_slot_deltas();
    let bank_snapshot_info = add_bank_snapshot(
        &temp_dir,
        bank,
        &snapshot_storages,
        snapshot_version,
        slot_deltas,
    )?;

    package_and_archive_full_snapshot(
        bank,
        &bank_snapshot_info,
        full_snapshot_archives_dir,
        incremental_snapshot_archives_dir,
        snapshot_storages,
        archive_format,
        snapshot_version,
        maximum_full_snapshot_archives_to_retain,
        maximum_incremental_snapshot_archives_to_retain,
    )
}

/// Convenience function to create an incremental snapshot archive out of any Bank, regardless of
/// state.  The Bank will be frozen during the process.
/// This is only called from ledger-tool or tests. Warping is a special case as well.
///
/// Requires:
///     - `bank` is complete
///     - `bank`'s slot is greater than `full_snapshot_slot`
pub fn bank_to_incremental_snapshot_archive(
    bank_snapshots_dir: impl AsRef<Path>,
    bank: &Bank,
    full_snapshot_slot: Slot,
    snapshot_version: Option<SnapshotVersion>,
    full_snapshot_archives_dir: impl AsRef<Path>,
    incremental_snapshot_archives_dir: impl AsRef<Path>,
    archive_format: ArchiveFormat,
    maximum_full_snapshot_archives_to_retain: NonZeroUsize,
    maximum_incremental_snapshot_archives_to_retain: NonZeroUsize,
) -> snapshot_utils::Result<IncrementalSnapshotArchiveInfo> {
    let snapshot_version = snapshot_version.unwrap_or_default();

    assert!(bank.is_complete());
    assert!(bank.slot() > full_snapshot_slot);
    bank.squash(); // Bank may not be a root
    bank.force_flush_accounts_cache();
    bank.clean_accounts(Some(full_snapshot_slot));
    if bank
        .feature_set
        .is_active(&feature_set::incremental_snapshot_only_incremental_hash_calculation::id())
    {
        bank.update_incremental_accounts_hash(full_snapshot_slot);
    } else {
        bank.update_accounts_hash(CalcAccountsHashDataSource::Storages, false, false);
    }
    bank.rehash(); // Bank accounts may have been manually modified by the caller

    let temp_dir = tempfile::tempdir_in(bank_snapshots_dir)?;
    let snapshot_storages = bank.get_snapshot_storages(Some(full_snapshot_slot));
    let slot_deltas = bank.status_cache.read().unwrap().root_slot_deltas();
    let bank_snapshot_info = add_bank_snapshot(
        &temp_dir,
        bank,
        &snapshot_storages,
        snapshot_version,
        slot_deltas,
    )?;

    package_and_archive_incremental_snapshot(
        bank,
        full_snapshot_slot,
        &bank_snapshot_info,
        full_snapshot_archives_dir,
        incremental_snapshot_archives_dir,
        snapshot_storages,
        archive_format,
        snapshot_version,
        maximum_full_snapshot_archives_to_retain,
        maximum_incremental_snapshot_archives_to_retain,
    )
}

/// Helper function to hold shared code to package, process, and archive full snapshots
#[allow(clippy::too_many_arguments)]
pub fn package_and_archive_full_snapshot(
    bank: &Bank,
    bank_snapshot_info: &BankSnapshotInfo,
    full_snapshot_archives_dir: impl AsRef<Path>,
    incremental_snapshot_archives_dir: impl AsRef<Path>,
    snapshot_storages: Vec<Arc<AccountStorageEntry>>,
    archive_format: ArchiveFormat,
    snapshot_version: SnapshotVersion,
    maximum_full_snapshot_archives_to_retain: NonZeroUsize,
    maximum_incremental_snapshot_archives_to_retain: NonZeroUsize,
) -> snapshot_utils::Result<FullSnapshotArchiveInfo> {
    let accounts_package = AccountsPackage::new_for_snapshot(
        AccountsPackageKind::Snapshot(SnapshotKind::FullSnapshot),
        bank,
        bank_snapshot_info,
        &full_snapshot_archives_dir,
        &incremental_snapshot_archives_dir,
        snapshot_storages,
        archive_format,
        snapshot_version,
        None,
    );

    let accounts_hash = bank
        .get_accounts_hash()
        .expect("accounts hash is required for snapshot");
    crate::serde_snapshot::reserialize_bank_with_new_accounts_hash(
        accounts_package.bank_snapshot_dir(),
        accounts_package.slot,
        &accounts_hash,
        None,
    );

    let snapshot_package = SnapshotPackage::new(accounts_package, accounts_hash.into());
    archive_snapshot_package(
        &snapshot_package,
        full_snapshot_archives_dir,
        incremental_snapshot_archives_dir,
        maximum_full_snapshot_archives_to_retain,
        maximum_incremental_snapshot_archives_to_retain,
    )?;

    Ok(FullSnapshotArchiveInfo::new(
        snapshot_package.snapshot_archive_info,
    ))
}

/// Helper function to hold shared code to package, process, and archive incremental snapshots
#[allow(clippy::too_many_arguments)]
pub fn package_and_archive_incremental_snapshot(
    bank: &Bank,
    incremental_snapshot_base_slot: Slot,
    bank_snapshot_info: &BankSnapshotInfo,
    full_snapshot_archives_dir: impl AsRef<Path>,
    incremental_snapshot_archives_dir: impl AsRef<Path>,
    snapshot_storages: Vec<Arc<AccountStorageEntry>>,
    archive_format: ArchiveFormat,
    snapshot_version: SnapshotVersion,
    maximum_full_snapshot_archives_to_retain: NonZeroUsize,
    maximum_incremental_snapshot_archives_to_retain: NonZeroUsize,
) -> snapshot_utils::Result<IncrementalSnapshotArchiveInfo> {
    let accounts_package = AccountsPackage::new_for_snapshot(
        AccountsPackageKind::Snapshot(SnapshotKind::IncrementalSnapshot(
            incremental_snapshot_base_slot,
        )),
        bank,
        bank_snapshot_info,
        &full_snapshot_archives_dir,
        &incremental_snapshot_archives_dir,
        snapshot_storages,
        archive_format,
        snapshot_version,
        None,
    );

    let (accounts_hash_kind, accounts_hash_for_reserialize, bank_incremental_snapshot_persistence) =
        if bank
            .feature_set
            .is_active(&feature_set::incremental_snapshot_only_incremental_hash_calculation::id())
        {
            let (base_accounts_hash, base_capitalization) = bank
                .rc
                .accounts
                .accounts_db
                .get_accounts_hash(incremental_snapshot_base_slot)
                .expect("base accounts hash is required for incremental snapshot");
            let (incremental_accounts_hash, incremental_capitalization) = bank
                .rc
                .accounts
                .accounts_db
                .get_incremental_accounts_hash(bank.slot())
                .expect("incremental accounts hash is required for incremental snapshot");
            let bank_incremental_snapshot_persistence = BankIncrementalSnapshotPersistence {
                full_slot: incremental_snapshot_base_slot,
                full_hash: base_accounts_hash.into(),
                full_capitalization: base_capitalization,
                incremental_hash: incremental_accounts_hash.into(),
                incremental_capitalization,
            };
            (
                incremental_accounts_hash.into(),
                AccountsHash(Hash::default()), // value does not matter; not used for incremental snapshots
                Some(bank_incremental_snapshot_persistence),
            )
        } else {
            let accounts_hash = bank
                .get_accounts_hash()
                .expect("accounts hash is required for snapshot");
            (accounts_hash.into(), accounts_hash, None)
        };

    crate::serde_snapshot::reserialize_bank_with_new_accounts_hash(
        accounts_package.bank_snapshot_dir(),
        accounts_package.slot,
        &accounts_hash_for_reserialize,
        bank_incremental_snapshot_persistence.as_ref(),
    );

    let snapshot_package = SnapshotPackage::new(accounts_package, accounts_hash_kind);
    archive_snapshot_package(
        &snapshot_package,
        full_snapshot_archives_dir,
        incremental_snapshot_archives_dir,
        maximum_full_snapshot_archives_to_retain,
        maximum_incremental_snapshot_archives_to_retain,
    )?;

    Ok(IncrementalSnapshotArchiveInfo::new(
        incremental_snapshot_base_slot,
        snapshot_package.snapshot_archive_info,
    ))
}

pub fn create_snapshot_dirs_for_tests(
    genesis_config: &GenesisConfig,
    bank_snapshots_dir: impl AsRef<Path>,
    num_total: usize,
    num_posts: usize,
) -> Bank {
    let mut bank = Arc::new(Bank::new_for_tests(genesis_config));

    let collecter_id = Pubkey::new_unique();
    let snapshot_version = SnapshotVersion::default();

    // loop to create the banks at slot 1 to num_total
    for _ in 0..num_total {
        // prepare the bank
        let slot = bank.slot() + 1;
        bank = Arc::new(Bank::new_from_parent(bank, &collecter_id, slot));
        bank.fill_bank_with_ticks_for_tests();
        bank.squash();
        bank.force_flush_accounts_cache();
        bank.update_accounts_hash(CalcAccountsHashDataSource::Storages, false, false);

        let snapshot_storages = bank.get_snapshot_storages(None);
        let slot_deltas = bank.status_cache.read().unwrap().root_slot_deltas();
        let bank_snapshot_info = add_bank_snapshot(
            &bank_snapshots_dir,
            &bank,
            &snapshot_storages,
            snapshot_version,
            slot_deltas,
        )
        .unwrap();

        if bank.slot() as usize > num_posts {
            continue; // leave the snapshot dir at PRE stage
        }

        // Reserialize the snapshot dir to convert it from PRE to POST, because only the POST type can be used
        // to construct a bank.
        assert!(
            crate::serde_snapshot::reserialize_bank_with_new_accounts_hash(
                &bank_snapshot_info.snapshot_dir,
                bank.slot(),
                &bank.get_accounts_hash().unwrap(),
                None
            )
        );
    }

    Arc::try_unwrap(bank).unwrap()
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            genesis_utils,
            snapshot_utils::{
                clean_orphaned_account_snapshot_dirs, create_all_accounts_run_and_snapshot_dirs,
                create_tmp_accounts_dir_for_tests, get_bank_snapshots, get_bank_snapshots_post,
                get_bank_snapshots_pre, get_highest_bank_snapshot, purge_bank_snapshot,
                purge_bank_snapshots_older_than_slot, purge_incomplete_bank_snapshots,
                purge_old_bank_snapshots, purge_old_bank_snapshots_at_startup,
                snapshot_storage_rebuilder::get_slot_and_append_vec_id, ArchiveFormat,
            },
            status_cache::Status,
        },
        solana_accounts_db::{
            accounts_db::ACCOUNTS_DB_CONFIG_FOR_TESTING,
            accounts_hash::{CalcAccountsHashConfig, HashStats},
            sorted_storages::SortedStorages,
        },
        solana_sdk::{
            genesis_config::create_genesis_config,
            native_token::{sol_to_lamports, LAMPORTS_PER_SOL},
            signature::{Keypair, Signer},
            system_transaction,
            transaction::SanitizedTransaction,
        },
        std::sync::{atomic::Ordering, Arc},
    };

    /// Test roundtrip of bank to a full snapshot, then back again.  This test creates the simplest
    /// bank possible, so the contents of the snapshot archive will be quite minimal.
    #[test]
    fn test_roundtrip_bank_to_and_from_full_snapshot_simple() {
        let genesis_config = GenesisConfig::default();
        let original_bank = Bank::new_for_tests(&genesis_config);

        while !original_bank.is_complete() {
            original_bank.register_tick(&Hash::new_unique());
        }

        let (_tmp_dir, accounts_dir) = create_tmp_accounts_dir_for_tests();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let snapshot_archive_format = ArchiveFormat::Tar;

        let snapshot_archive_info = bank_to_full_snapshot_archive(
            &bank_snapshots_dir,
            &original_bank,
            None,
            full_snapshot_archives_dir.path(),
            incremental_snapshot_archives_dir.path(),
            snapshot_archive_format,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let (roundtrip_bank, _) = bank_from_snapshot_archives(
            &[accounts_dir],
            bank_snapshots_dir.path(),
            &snapshot_archive_info,
            None,
            &genesis_config,
            &RuntimeConfig::default(),
            None,
            None,
            AccountSecondaryIndexes::default(),
            None,
            AccountShrinkThreshold::default(),
            false,
            false,
            false,
            false,
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
        )
        .unwrap();
        roundtrip_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();
        assert_eq!(original_bank, roundtrip_bank);
    }

    /// Test roundtrip of bank to a full snapshot, then back again.  This test is more involved
    /// than the simple version above; creating multiple banks over multiple slots and doing
    /// multiple transfers.  So this full snapshot should contain more data.
    #[test]
    fn test_roundtrip_bank_to_and_from_snapshot_complex() {
        let collector = Pubkey::new_unique();
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let key3 = Keypair::new();
        let key4 = Keypair::new();
        let key5 = Keypair::new();

        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1_000_000.));
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        bank0
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        bank0
            .transfer(sol_to_lamports(2.), &mint_keypair, &key2.pubkey())
            .unwrap();
        bank0
            .transfer(sol_to_lamports(3.), &mint_keypair, &key3.pubkey())
            .unwrap();
        while !bank0.is_complete() {
            bank0.register_tick(&Hash::new_unique());
        }

        let slot = 1;
        let bank1 = Arc::new(Bank::new_from_parent(bank0, &collector, slot));
        bank1
            .transfer(sol_to_lamports(3.), &mint_keypair, &key3.pubkey())
            .unwrap();
        bank1
            .transfer(sol_to_lamports(4.), &mint_keypair, &key4.pubkey())
            .unwrap();
        bank1
            .transfer(sol_to_lamports(5.), &mint_keypair, &key5.pubkey())
            .unwrap();
        while !bank1.is_complete() {
            bank1.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank2 = Arc::new(Bank::new_from_parent(bank1, &collector, slot));
        bank2
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        while !bank2.is_complete() {
            bank2.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank3 = Arc::new(Bank::new_from_parent(bank2, &collector, slot));
        bank3
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        while !bank3.is_complete() {
            bank3.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank4 = Arc::new(Bank::new_from_parent(bank3, &collector, slot));
        bank4
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        while !bank4.is_complete() {
            bank4.register_tick(&Hash::new_unique());
        }

        let (_tmp_dir, accounts_dir) = create_tmp_accounts_dir_for_tests();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let snapshot_archive_format = ArchiveFormat::TarGzip;

        let full_snapshot_archive_info = bank_to_full_snapshot_archive(
            bank_snapshots_dir.path(),
            &bank4,
            None,
            full_snapshot_archives_dir.path(),
            incremental_snapshot_archives_dir.path(),
            snapshot_archive_format,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let (roundtrip_bank, _) = bank_from_snapshot_archives(
            &[accounts_dir],
            bank_snapshots_dir.path(),
            &full_snapshot_archive_info,
            None,
            &genesis_config,
            &RuntimeConfig::default(),
            None,
            None,
            AccountSecondaryIndexes::default(),
            None,
            AccountShrinkThreshold::default(),
            false,
            false,
            false,
            false,
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
        )
        .unwrap();
        roundtrip_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();
        assert_eq!(*bank4, roundtrip_bank);
    }

    /// Test roundtrip of bank to snapshots, then back again, with incremental snapshots.  In this
    /// version, build up a few slots and take a full snapshot.  Continue on a few more slots and
    /// take an incremental snapshot.  Rebuild the bank from both the incremental snapshot and full
    /// snapshot.
    ///
    /// For the full snapshot, touch all the accounts, but only one for the incremental snapshot.
    /// This is intended to mimic the real behavior of transactions, where only a small number of
    /// accounts are modified often, which are captured by the incremental snapshot.  The majority
    /// of the accounts are not modified often, and are captured by the full snapshot.
    #[test]
    fn test_roundtrip_bank_to_and_from_incremental_snapshot() {
        let collector = Pubkey::new_unique();
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let key3 = Keypair::new();
        let key4 = Keypair::new();
        let key5 = Keypair::new();

        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1_000_000.));
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        bank0
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        bank0
            .transfer(sol_to_lamports(2.), &mint_keypair, &key2.pubkey())
            .unwrap();
        bank0
            .transfer(sol_to_lamports(3.), &mint_keypair, &key3.pubkey())
            .unwrap();
        while !bank0.is_complete() {
            bank0.register_tick(&Hash::new_unique());
        }

        let slot = 1;
        let bank1 = Arc::new(Bank::new_from_parent(bank0, &collector, slot));
        bank1
            .transfer(sol_to_lamports(3.), &mint_keypair, &key3.pubkey())
            .unwrap();
        bank1
            .transfer(sol_to_lamports(4.), &mint_keypair, &key4.pubkey())
            .unwrap();
        bank1
            .transfer(sol_to_lamports(5.), &mint_keypair, &key5.pubkey())
            .unwrap();
        while !bank1.is_complete() {
            bank1.register_tick(&Hash::new_unique());
        }

        let (_tmp_dir, accounts_dir) = create_tmp_accounts_dir_for_tests();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let snapshot_archive_format = ArchiveFormat::TarZstd;

        let full_snapshot_slot = slot;
        let full_snapshot_archive_info = bank_to_full_snapshot_archive(
            bank_snapshots_dir.path(),
            &bank1,
            None,
            full_snapshot_archives_dir.path(),
            incremental_snapshot_archives_dir.path(),
            snapshot_archive_format,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let slot = slot + 1;
        let bank2 = Arc::new(Bank::new_from_parent(bank1, &collector, slot));
        bank2
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        while !bank2.is_complete() {
            bank2.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank3 = Arc::new(Bank::new_from_parent(bank2, &collector, slot));
        bank3
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        while !bank3.is_complete() {
            bank3.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank4 = Arc::new(Bank::new_from_parent(bank3, &collector, slot));
        bank4
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        while !bank4.is_complete() {
            bank4.register_tick(&Hash::new_unique());
        }

        let incremental_snapshot_archive_info = bank_to_incremental_snapshot_archive(
            bank_snapshots_dir.path(),
            &bank4,
            full_snapshot_slot,
            None,
            full_snapshot_archives_dir.path(),
            incremental_snapshot_archives_dir.path(),
            snapshot_archive_format,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let (roundtrip_bank, _) = bank_from_snapshot_archives(
            &[accounts_dir],
            bank_snapshots_dir.path(),
            &full_snapshot_archive_info,
            Some(&incremental_snapshot_archive_info),
            &genesis_config,
            &RuntimeConfig::default(),
            None,
            None,
            AccountSecondaryIndexes::default(),
            None,
            AccountShrinkThreshold::default(),
            false,
            false,
            false,
            false,
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
        )
        .unwrap();
        roundtrip_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();
        assert_eq!(*bank4, roundtrip_bank);
    }

    /// Test rebuilding bank from the latest snapshot archives
    #[test]
    fn test_bank_from_latest_snapshot_archives() {
        let collector = Pubkey::new_unique();
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let key3 = Keypair::new();

        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1_000_000.));
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        bank0
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        bank0
            .transfer(sol_to_lamports(2.), &mint_keypair, &key2.pubkey())
            .unwrap();
        bank0
            .transfer(sol_to_lamports(3.), &mint_keypair, &key3.pubkey())
            .unwrap();
        while !bank0.is_complete() {
            bank0.register_tick(&Hash::new_unique());
        }

        let slot = 1;
        let bank1 = Arc::new(Bank::new_from_parent(bank0, &collector, slot));
        bank1
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        bank1
            .transfer(sol_to_lamports(2.), &mint_keypair, &key2.pubkey())
            .unwrap();
        bank1
            .transfer(sol_to_lamports(3.), &mint_keypair, &key3.pubkey())
            .unwrap();
        while !bank1.is_complete() {
            bank1.register_tick(&Hash::new_unique());
        }

        let (_tmp_dir, accounts_dir) = create_tmp_accounts_dir_for_tests();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let snapshot_archive_format = ArchiveFormat::Tar;

        let full_snapshot_slot = slot;
        bank_to_full_snapshot_archive(
            &bank_snapshots_dir,
            &bank1,
            None,
            &full_snapshot_archives_dir,
            &incremental_snapshot_archives_dir,
            snapshot_archive_format,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let slot = slot + 1;
        let bank2 = Arc::new(Bank::new_from_parent(bank1, &collector, slot));
        bank2
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        while !bank2.is_complete() {
            bank2.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank3 = Arc::new(Bank::new_from_parent(bank2, &collector, slot));
        bank3
            .transfer(sol_to_lamports(2.), &mint_keypair, &key2.pubkey())
            .unwrap();
        while !bank3.is_complete() {
            bank3.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank4 = Arc::new(Bank::new_from_parent(bank3, &collector, slot));
        bank4
            .transfer(sol_to_lamports(3.), &mint_keypair, &key3.pubkey())
            .unwrap();
        while !bank4.is_complete() {
            bank4.register_tick(&Hash::new_unique());
        }

        bank_to_incremental_snapshot_archive(
            &bank_snapshots_dir,
            &bank4,
            full_snapshot_slot,
            None,
            &full_snapshot_archives_dir,
            &incremental_snapshot_archives_dir,
            snapshot_archive_format,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let (deserialized_bank, ..) = bank_from_latest_snapshot_archives(
            &bank_snapshots_dir,
            &full_snapshot_archives_dir,
            &incremental_snapshot_archives_dir,
            &[accounts_dir],
            &genesis_config,
            &RuntimeConfig::default(),
            None,
            None,
            AccountSecondaryIndexes::default(),
            None,
            AccountShrinkThreshold::default(),
            false,
            false,
            false,
            false,
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
        )
        .unwrap();
        deserialized_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();
        assert_eq!(deserialized_bank, *bank4);
    }

    /// Test that cleaning works well in the edge cases of zero-lamport accounts and snapshots.
    /// Here's the scenario:
    ///
    /// slot 1:
    ///     - send some lamports to Account1 (from Account2) to bring it to life
    ///     - take a full snapshot
    /// slot 2:
    ///     - make Account1 have zero lamports (send back to Account2)
    ///     - take an incremental snapshot
    ///     - ensure deserializing from this snapshot is equal to this bank
    /// slot 3:
    ///     - remove Account2's reference back to slot 2 by transfering from the mint to Account2
    /// slot 4:
    ///     - ensure `clean_accounts()` has run and that Account1 is gone
    ///     - take another incremental snapshot
    ///     - ensure deserializing from this snapshots is equal to this bank
    ///     - ensure Account1 hasn't come back from the dead
    ///
    /// The check at slot 4 will fail with the pre-incremental-snapshot cleaning logic.  Because
    /// of the cleaning/purging at slot 4, the incremental snapshot at slot 4 will no longer have
    /// information about Account1, but the full snapshost _does_ have info for Account1, which is
    /// no longer correct!
    #[test]
    fn test_incremental_snapshots_handle_zero_lamport_accounts() {
        let collector = Pubkey::new_unique();
        let key1 = Keypair::new();
        let key2 = Keypair::new();

        let (_tmp_dir, accounts_dir) = create_tmp_accounts_dir_for_tests();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let snapshot_archive_format = ArchiveFormat::Tar;

        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1_000_000.));

        let lamports_to_transfer = sol_to_lamports(123_456.);
        let bank0 = Arc::new(Bank::new_with_paths_for_tests(
            &genesis_config,
            Arc::<RuntimeConfig>::default(),
            vec![accounts_dir.clone()],
            AccountSecondaryIndexes::default(),
            AccountShrinkThreshold::default(),
        ));
        bank0
            .transfer(lamports_to_transfer, &mint_keypair, &key2.pubkey())
            .unwrap();
        while !bank0.is_complete() {
            bank0.register_tick(&Hash::new_unique());
        }

        let slot = 1;
        let bank1 = Arc::new(Bank::new_from_parent(bank0, &collector, slot));
        bank1
            .transfer(lamports_to_transfer, &key2, &key1.pubkey())
            .unwrap();
        while !bank1.is_complete() {
            bank1.register_tick(&Hash::new_unique());
        }

        let full_snapshot_slot = slot;
        let full_snapshot_archive_info = bank_to_full_snapshot_archive(
            bank_snapshots_dir.path(),
            &bank1,
            None,
            full_snapshot_archives_dir.path(),
            incremental_snapshot_archives_dir.path(),
            snapshot_archive_format,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let slot = slot + 1;
        let bank2 = Arc::new(Bank::new_from_parent(bank1, &collector, slot));
        let blockhash = bank2.last_blockhash();
        let tx = SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &key1,
            &key2.pubkey(),
            lamports_to_transfer,
            blockhash,
        ));
        let fee = bank2.get_fee_for_message(tx.message()).unwrap();
        let tx = system_transaction::transfer(
            &key1,
            &key2.pubkey(),
            lamports_to_transfer - fee,
            blockhash,
        );
        bank2.process_transaction(&tx).unwrap();
        assert_eq!(
            bank2.get_balance(&key1.pubkey()),
            0,
            "Ensure Account1's balance is zero"
        );
        while !bank2.is_complete() {
            bank2.register_tick(&Hash::new_unique());
        }

        // Take an incremental snapshot and then do a roundtrip on the bank and ensure it
        // deserializes correctly.
        let incremental_snapshot_archive_info = bank_to_incremental_snapshot_archive(
            bank_snapshots_dir.path(),
            &bank2,
            full_snapshot_slot,
            None,
            full_snapshot_archives_dir.path(),
            incremental_snapshot_archives_dir.path(),
            snapshot_archive_format,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();
        let (deserialized_bank, _) = bank_from_snapshot_archives(
            &[accounts_dir.clone()],
            bank_snapshots_dir.path(),
            &full_snapshot_archive_info,
            Some(&incremental_snapshot_archive_info),
            &genesis_config,
            &RuntimeConfig::default(),
            None,
            None,
            AccountSecondaryIndexes::default(),
            None,
            AccountShrinkThreshold::default(),
            false,
            false,
            false,
            false,
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
        )
        .unwrap();
        deserialized_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();
        assert_eq!(
            deserialized_bank, *bank2,
            "Ensure rebuilding from an incremental snapshot works"
        );

        let slot = slot + 1;
        let bank3 = Arc::new(Bank::new_from_parent(bank2, &collector, slot));
        // Update Account2 so that it no longer holds a reference to slot2
        bank3
            .transfer(lamports_to_transfer, &mint_keypair, &key2.pubkey())
            .unwrap();
        while !bank3.is_complete() {
            bank3.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank4 = Arc::new(Bank::new_from_parent(bank3, &collector, slot));
        while !bank4.is_complete() {
            bank4.register_tick(&Hash::new_unique());
        }

        // Ensure account1 has been cleaned/purged from everywhere
        bank4.squash();
        bank4.clean_accounts(Some(full_snapshot_slot));
        assert!(
            bank4.get_account_modified_slot(&key1.pubkey()).is_none(),
            "Ensure Account1 has been cleaned and purged from AccountsDb"
        );

        // Take an incremental snapshot and then do a roundtrip on the bank and ensure it
        // deserializes correctly
        let incremental_snapshot_archive_info = bank_to_incremental_snapshot_archive(
            bank_snapshots_dir.path(),
            &bank4,
            full_snapshot_slot,
            None,
            full_snapshot_archives_dir.path(),
            incremental_snapshot_archives_dir.path(),
            snapshot_archive_format,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let (deserialized_bank, _) = bank_from_snapshot_archives(
            &[accounts_dir],
            bank_snapshots_dir.path(),
            &full_snapshot_archive_info,
            Some(&incremental_snapshot_archive_info),
            &genesis_config,
            &RuntimeConfig::default(),
            None,
            None,
            AccountSecondaryIndexes::default(),
            None,
            AccountShrinkThreshold::default(),
            false,
            false,
            false,
            false,
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
        )
        .unwrap();
        deserialized_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();
        assert_eq!(
            deserialized_bank, *bank4,
            "Ensure rebuilding from an incremental snapshot works",
        );
        assert!(
            deserialized_bank
                .get_account_modified_slot(&key1.pubkey())
                .is_none(),
            "Ensure Account1 has not been brought back from the dead"
        );
    }

    #[test]
    fn test_bank_fields_from_snapshot() {
        let collector = Pubkey::new_unique();
        let key1 = Keypair::new();

        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1_000_000.));
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        while !bank0.is_complete() {
            bank0.register_tick(&Hash::new_unique());
        }

        let slot = 1;
        let bank1 = Arc::new(Bank::new_from_parent(bank0, &collector, slot));
        while !bank1.is_complete() {
            bank1.register_tick(&Hash::new_unique());
        }

        let all_snapshots_dir = tempfile::TempDir::new().unwrap();
        let snapshot_archive_format = ArchiveFormat::Tar;

        let full_snapshot_slot = slot;
        bank_to_full_snapshot_archive(
            &all_snapshots_dir,
            &bank1,
            None,
            &all_snapshots_dir,
            &all_snapshots_dir,
            snapshot_archive_format,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let slot = slot + 1;
        let bank2 = Arc::new(Bank::new_from_parent(bank1, &collector, slot));
        bank2
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        while !bank2.is_complete() {
            bank2.register_tick(&Hash::new_unique());
        }

        bank_to_incremental_snapshot_archive(
            &all_snapshots_dir,
            &bank2,
            full_snapshot_slot,
            None,
            &all_snapshots_dir,
            &all_snapshots_dir,
            snapshot_archive_format,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let bank_fields =
            bank_fields_from_snapshot_archives(&all_snapshots_dir, &all_snapshots_dir).unwrap();
        assert_eq!(bank_fields.slot, bank2.slot());
        assert_eq!(bank_fields.parent_slot, bank2.parent_slot());
    }

    #[test]
    fn test_bank_snapshot_dir_accounts_hardlinks() {
        let genesis_config = GenesisConfig::default();
        let bank = Bank::new_for_tests(&genesis_config);

        bank.fill_bank_with_ticks_for_tests();

        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();

        bank.squash();
        bank.force_flush_accounts_cache();

        let snapshot_version = SnapshotVersion::default();
        let snapshot_storages = bank.get_snapshot_storages(None);
        let slot_deltas = bank.status_cache.read().unwrap().root_slot_deltas();
        add_bank_snapshot(
            &bank_snapshots_dir,
            &bank,
            &snapshot_storages,
            snapshot_version,
            slot_deltas,
        )
        .unwrap();

        let accounts_hardlinks_dir = get_bank_snapshot_dir(&bank_snapshots_dir, bank.slot())
            .join(snapshot_utils::SNAPSHOT_ACCOUNTS_HARDLINKS);
        assert!(fs_err::metadata(&accounts_hardlinks_dir).is_ok());

        let mut hardlink_dirs: Vec<PathBuf> = Vec::new();
        // This directory contain symlinks to all accounts snapshot directories.
        for entry in fs_err::read_dir(accounts_hardlinks_dir).unwrap() {
            let entry = entry.unwrap();
            let symlink = entry.path();
            let dst_path = fs_err::read_link(symlink).unwrap();
            assert!(fs_err::metadata(&dst_path).is_ok());
            hardlink_dirs.push(dst_path);
        }

        let bank_snapshot_dir = get_bank_snapshot_dir(&bank_snapshots_dir, bank.slot());
        assert!(purge_bank_snapshot(bank_snapshot_dir).is_ok());

        // When the bank snapshot is removed, all the snapshot hardlink directories should be removed.
        assert!(hardlink_dirs
            .iter()
            .all(|dir| fs_err::metadata(dir).is_err()));
    }

    #[test]
    fn test_get_highest_bank_snapshot() {
        let genesis_config = GenesisConfig::default();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let _bank = create_snapshot_dirs_for_tests(&genesis_config, &bank_snapshots_dir, 4, 0);

        let snapshot = get_highest_bank_snapshot(&bank_snapshots_dir).unwrap();
        assert_eq!(snapshot.slot, 4);

        let complete_flag_file = snapshot
            .snapshot_dir
            .join(snapshot_utils::SNAPSHOT_STATE_COMPLETE_FILENAME);
        fs_err::remove_file(complete_flag_file).unwrap();
        // The incomplete snapshot dir should still exist
        let snapshot_dir_4 = snapshot.snapshot_dir;
        assert!(snapshot_dir_4.exists());
        let snapshot = get_highest_bank_snapshot(&bank_snapshots_dir).unwrap();
        assert_eq!(snapshot.slot, 3);

        let snapshot_version_file = snapshot
            .snapshot_dir
            .join(snapshot_utils::SNAPSHOT_VERSION_FILENAME);
        fs_err::remove_file(snapshot_version_file).unwrap();
        let snapshot = get_highest_bank_snapshot(&bank_snapshots_dir).unwrap();
        assert_eq!(snapshot.slot, 2);

        let status_cache_file = snapshot
            .snapshot_dir
            .join(snapshot_utils::SNAPSHOT_STATUS_CACHE_FILENAME);
        fs_err::remove_file(status_cache_file).unwrap();
        let snapshot = get_highest_bank_snapshot(&bank_snapshots_dir).unwrap();
        assert_eq!(snapshot.slot, 1);
    }

    #[test]
    pub fn test_create_all_accounts_run_and_snapshot_dirs() {
        let (_tmp_dirs, account_paths): (Vec<TempDir>, Vec<PathBuf>) = (0..4)
            .map(|_| {
                let tmp_dir = tempfile::TempDir::new().unwrap();
                let account_path = tmp_dir.path().join("accounts");
                (tmp_dir, account_path)
            })
            .unzip();

        // create the `run/` and `snapshot/` dirs, and ensure they're there
        let (account_run_paths, account_snapshot_paths) =
            create_all_accounts_run_and_snapshot_dirs(&account_paths).unwrap();
        account_run_paths.iter().all(|path| path.is_dir());
        account_snapshot_paths.iter().all(|path| path.is_dir());

        // delete a `run/` and `snapshot/` dir, then re-create it
        let account_path_first = account_paths.first().unwrap();
        delete_contents_of_path(account_path_first);
        assert!(account_path_first.exists());
        assert!(!account_path_first.join("run").exists());
        assert!(!account_path_first.join("snapshot").exists());

        _ = create_all_accounts_run_and_snapshot_dirs(&account_paths).unwrap();
        account_run_paths.iter().all(|path| path.is_dir());
        account_snapshot_paths.iter().all(|path| path.is_dir());
    }

    #[test]
    fn test_clean_orphaned_account_snapshot_dirs() {
        let genesis_config = GenesisConfig::default();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let _bank = create_snapshot_dirs_for_tests(&genesis_config, &bank_snapshots_dir, 2, 0);

        let snapshot_dir_slot_2 = bank_snapshots_dir.path().join("2");
        let accounts_link_dir_slot_2 =
            snapshot_dir_slot_2.join(snapshot_utils::SNAPSHOT_ACCOUNTS_HARDLINKS);

        // the symlinks point to the account snapshot hardlink directories <account_path>/snapshot/<slot>/ for slot 2
        // get them via read_link
        let hardlink_dirs_slot_2: Vec<PathBuf> = fs_err::read_dir(accounts_link_dir_slot_2)
            .unwrap()
            .map(|entry| {
                let symlink = entry.unwrap().path();
                fs_err::read_link(symlink).unwrap()
            })
            .collect();

        // remove the bank snapshot directory for slot 2, so the account snapshot slot 2 directories become orphaned
        fs_err::remove_dir_all(snapshot_dir_slot_2).unwrap();

        // verify the orphaned account snapshot hardlink directories are still there
        assert!(hardlink_dirs_slot_2
            .iter()
            .all(|dir| fs_err::metadata(dir).is_ok()));

        let account_snapshot_paths: Vec<PathBuf> = hardlink_dirs_slot_2
            .iter()
            .map(|dir| dir.parent().unwrap().parent().unwrap().to_path_buf())
            .collect();
        // clean the orphaned hardlink directories
        clean_orphaned_account_snapshot_dirs(&bank_snapshots_dir, &account_snapshot_paths).unwrap();

        // verify the hardlink directories are gone
        assert!(hardlink_dirs_slot_2
            .iter()
            .all(|dir| fs_err::metadata(dir).is_err()));
    }

    #[test]
    fn test_purge_incomplete_bank_snapshots() {
        let genesis_config = GenesisConfig::default();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let _bank = create_snapshot_dirs_for_tests(&genesis_config, &bank_snapshots_dir, 2, 0);

        // remove the "state complete" files so the snapshots will be purged
        for slot in [1, 2] {
            let bank_snapshot_dir = get_bank_snapshot_dir(&bank_snapshots_dir, slot);
            let state_complete_file =
                bank_snapshot_dir.join(snapshot_utils::SNAPSHOT_STATE_COMPLETE_FILENAME);
            fs_err::remove_file(state_complete_file).unwrap();
        }

        purge_incomplete_bank_snapshots(&bank_snapshots_dir);

        // ensure the bank snapshots dirs are gone
        for slot in [1, 2] {
            let bank_snapshot_dir = get_bank_snapshot_dir(&bank_snapshots_dir, slot);
            assert!(!bank_snapshot_dir.exists());
        }
    }

    /// Test that snapshots with the Incremental Accounts Hash feature enabled can roundtrip.
    ///
    /// This test generates banks with zero and non-zero lamport accounts then takes full and
    /// incremental snapshots.  A bank is deserialized from the snapshots, its incremental
    /// accounts hash is recalculated, and then compared with the original.
    #[test]
    fn test_incremental_snapshot_with_incremental_accounts_hash() {
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();

        let genesis_config_info = genesis_utils::create_genesis_config_with_leader(
            1_000_000 * LAMPORTS_PER_SOL,
            &Pubkey::new_unique(),
            100 * LAMPORTS_PER_SOL,
        );
        let mint = &genesis_config_info.mint_keypair;

        let do_transfers = |bank: &Bank| {
            let key1 = Keypair::new(); // lamports from mint
            let key2 = Keypair::new(); // will end with ZERO lamports
            let key3 = Keypair::new(); // lamports from key2

            let amount = 123_456_789;
            let fee = {
                let blockhash = bank.last_blockhash();
                let transaction = SanitizedTransaction::from_transaction_for_tests(
                    system_transaction::transfer(&key2, &key3.pubkey(), amount, blockhash),
                );
                bank.get_fee_for_message(transaction.message()).unwrap()
            };
            bank.transfer(amount + fee, mint, &key1.pubkey()).unwrap();
            bank.transfer(amount + fee, mint, &key2.pubkey()).unwrap();
            bank.transfer(amount + fee, &key2, &key3.pubkey()).unwrap();
            assert_eq!(bank.get_balance(&key2.pubkey()), 0);

            bank.fill_bank_with_ticks_for_tests();
        };

        let mut bank = Arc::new(Bank::new_for_tests(&genesis_config_info.genesis_config));

        // make some banks, do some transactions, ensure there's some zero-lamport accounts
        for _ in 0..5 {
            let slot = bank.slot() + 1;
            bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::new_unique(), slot));
            do_transfers(&bank);
        }

        // take full snapshot, save off the calculated accounts hash
        let full_snapshot_archive = bank_to_full_snapshot_archive(
            &bank_snapshots_dir,
            &bank,
            None,
            &full_snapshot_archives_dir,
            &incremental_snapshot_archives_dir,
            ArchiveFormat::Tar,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();
        let full_accounts_hash = bank
            .rc
            .accounts
            .accounts_db
            .get_accounts_hash(bank.slot())
            .unwrap();

        // make more banks, do more transactions, ensure there's more zero-lamport accounts
        for _ in 0..5 {
            let slot = bank.slot() + 1;
            bank = Arc::new(Bank::new_from_parent(bank, &Pubkey::new_unique(), slot));
            do_transfers(&bank);
        }

        // take incremental snapshot, save off the calculated incremental accounts hash
        let incremental_snapshot_archive = bank_to_incremental_snapshot_archive(
            &bank_snapshots_dir,
            &bank,
            full_snapshot_archive.slot(),
            None,
            &full_snapshot_archives_dir,
            &incremental_snapshot_archives_dir,
            ArchiveFormat::Tar,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();
        let incremental_accounts_hash = bank
            .rc
            .accounts
            .accounts_db
            .get_incremental_accounts_hash(bank.slot())
            .unwrap();

        // reconstruct a bank from the snapshots
        let other_accounts_dir = tempfile::TempDir::new().unwrap();
        let other_bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let (deserialized_bank, _) = bank_from_snapshot_archives(
            &[other_accounts_dir.path().to_path_buf()],
            &other_bank_snapshots_dir,
            &full_snapshot_archive,
            Some(&incremental_snapshot_archive),
            &genesis_config_info.genesis_config,
            &RuntimeConfig::default(),
            None,
            None,
            AccountSecondaryIndexes::default(),
            None,
            AccountShrinkThreshold::default(),
            false,
            false,
            false,
            false,
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
        )
        .unwrap();
        deserialized_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();
        assert_eq!(&deserialized_bank, bank.as_ref());

        // ensure the accounts hash stored in the deserialized bank matches
        let deserialized_accounts_hash = deserialized_bank
            .rc
            .accounts
            .accounts_db
            .get_accounts_hash(full_snapshot_archive.slot())
            .unwrap();
        assert_eq!(deserialized_accounts_hash, full_accounts_hash);

        // ensure the incremental accounts hash stored in the deserialized bank matches
        let deserialized_incrmental_accounts_hash = deserialized_bank
            .rc
            .accounts
            .accounts_db
            .get_incremental_accounts_hash(incremental_snapshot_archive.slot())
            .unwrap();
        assert_eq!(
            deserialized_incrmental_accounts_hash,
            incremental_accounts_hash
        );

        // recalculate the incremental accounts hash on the desserialized bank and ensure it matches
        let other_incremental_snapshot_storages =
            deserialized_bank.get_snapshot_storages(Some(full_snapshot_archive.slot()));
        let other_incremental_accounts_hash = bank
            .rc
            .accounts
            .accounts_db
            .calculate_incremental_accounts_hash(
                &CalcAccountsHashConfig {
                    use_bg_thread_pool: false,
                    check_hash: false,
                    ancestors: None,
                    epoch_schedule: deserialized_bank.epoch_schedule(),
                    rent_collector: deserialized_bank.rent_collector(),
                    store_detailed_debug_info_on_failure: false,
                    include_slot_in_hash: bank.include_slot_in_hash(),
                },
                &SortedStorages::new(&other_incremental_snapshot_storages),
                HashStats::default(),
            )
            .unwrap();
        assert_eq!(other_incremental_accounts_hash, incremental_accounts_hash);
    }

    #[test]
    fn test_bank_from_snapshot_dir() {
        let genesis_config = GenesisConfig::default();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let bank = create_snapshot_dirs_for_tests(&genesis_config, &bank_snapshots_dir, 3, 0);

        let bank_snapshot = get_highest_bank_snapshot(&bank_snapshots_dir).unwrap();
        let account_paths = &bank.rc.accounts.accounts_db.paths;

        let (bank_constructed, ..) = bank_from_snapshot_dir(
            account_paths,
            &bank_snapshot,
            &genesis_config,
            &RuntimeConfig::default(),
            None,
            None,
            AccountSecondaryIndexes::default(),
            None,
            AccountShrinkThreshold::default(),
            false,
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
        )
        .unwrap();

        bank_constructed.wait_for_initial_accounts_hash_verification_completed_for_tests();
        assert_eq!(bank_constructed, bank);

        // Verify that the next_append_vec_id tracking is correct
        let mut max_id = 0;
        for path in account_paths {
            fs_err::read_dir(path).unwrap().for_each(|entry| {
                let path = entry.unwrap().path();
                let filename = path.file_name().unwrap();
                let (_slot, append_vec_id) = get_slot_and_append_vec_id(filename.to_str().unwrap());
                max_id = std::cmp::max(max_id, append_vec_id);
            });
        }
        let next_id = bank.accounts().accounts_db.next_id.load(Ordering::Relaxed) as usize;
        assert_eq!(max_id, next_id - 1);
    }

    #[test]
    fn test_bank_from_latest_snapshot_dir() {
        let genesis_config = GenesisConfig::default();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let bank = create_snapshot_dirs_for_tests(&genesis_config, &bank_snapshots_dir, 3, 3);

        let account_paths = &bank.rc.accounts.accounts_db.paths;

        let deserialized_bank = bank_from_latest_snapshot_dir(
            &bank_snapshots_dir,
            &genesis_config,
            &RuntimeConfig::default(),
            account_paths,
            None,
            None,
            AccountSecondaryIndexes::default(),
            None,
            AccountShrinkThreshold::default(),
            false,
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            Arc::default(),
        )
        .unwrap();

        assert_eq!(
            deserialized_bank, bank,
            "Ensure rebuilding bank from the highest snapshot dir results in the highest bank",
        );
    }

    #[test]
    fn test_purge_old_bank_snapshots() {
        let genesis_config = GenesisConfig::default();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let _bank = create_snapshot_dirs_for_tests(&genesis_config, &bank_snapshots_dir, 10, 5);
        // Keep bank in this scope so that its account_paths tmp dirs are not released, and purge_old_bank_snapshots
        // can clear the account hardlinks correctly.

        assert_eq!(get_bank_snapshots(&bank_snapshots_dir).len(), 10);

        purge_old_bank_snapshots(&bank_snapshots_dir, 3, Some(BankSnapshotType::Pre));
        assert_eq!(get_bank_snapshots_pre(&bank_snapshots_dir).len(), 3);

        purge_old_bank_snapshots(&bank_snapshots_dir, 2, Some(BankSnapshotType::Post));
        assert_eq!(get_bank_snapshots_post(&bank_snapshots_dir).len(), 2);

        assert_eq!(get_bank_snapshots(&bank_snapshots_dir).len(), 5);

        purge_old_bank_snapshots(&bank_snapshots_dir, 2, None);
        assert_eq!(get_bank_snapshots(&bank_snapshots_dir).len(), 2);

        purge_old_bank_snapshots(&bank_snapshots_dir, 0, None);
        assert_eq!(get_bank_snapshots(&bank_snapshots_dir).len(), 0);
    }

    #[test]
    fn test_purge_bank_snapshots_older_than_slot() {
        let genesis_config = GenesisConfig::default();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();

        // The bank must stay in scope to ensure the temp dirs that it holds are not dropped
        let _bank = create_snapshot_dirs_for_tests(&genesis_config, &bank_snapshots_dir, 9, 6);
        let bank_snapshots_before = get_bank_snapshots(&bank_snapshots_dir);

        purge_bank_snapshots_older_than_slot(&bank_snapshots_dir, 0);
        let bank_snapshots_after = get_bank_snapshots(&bank_snapshots_dir);
        assert_eq!(bank_snapshots_before.len(), bank_snapshots_after.len());

        purge_bank_snapshots_older_than_slot(&bank_snapshots_dir, 3);
        let bank_snapshots_after = get_bank_snapshots(&bank_snapshots_dir);
        assert_eq!(bank_snapshots_before.len(), bank_snapshots_after.len() + 2);

        purge_bank_snapshots_older_than_slot(&bank_snapshots_dir, 8);
        let bank_snapshots_after = get_bank_snapshots(&bank_snapshots_dir);
        assert_eq!(bank_snapshots_before.len(), bank_snapshots_after.len() + 7);

        purge_bank_snapshots_older_than_slot(&bank_snapshots_dir, Slot::MAX);
        let bank_snapshots_after = get_bank_snapshots(&bank_snapshots_dir);
        assert_eq!(bank_snapshots_before.len(), bank_snapshots_after.len() + 9);
        assert!(bank_snapshots_after.is_empty());
    }

    #[test]
    fn test_purge_old_bank_snapshots_at_startup() {
        let genesis_config = GenesisConfig::default();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();

        // The bank must stay in scope to ensure the temp dirs that it holds are not dropped
        let _bank = create_snapshot_dirs_for_tests(&genesis_config, &bank_snapshots_dir, 9, 6);

        purge_old_bank_snapshots_at_startup(&bank_snapshots_dir);

        let bank_snapshots_pre = get_bank_snapshots_pre(&bank_snapshots_dir);
        assert!(bank_snapshots_pre.is_empty());

        let bank_snapshots_post = get_bank_snapshots_post(&bank_snapshots_dir);
        assert_eq!(bank_snapshots_post.len(), 1);
        assert_eq!(bank_snapshots_post.first().unwrap().slot, 6);
    }

    #[test]
    fn test_verify_slot_deltas_structural_bad_too_many_entries() {
        let bank_slot = status_cache::MAX_CACHE_ENTRIES as Slot + 1;
        let slot_deltas: Vec<_> = (0..bank_slot)
            .map(|slot| (slot, true, Status::default()))
            .collect();

        let result = verify_slot_deltas_structural(slot_deltas.as_slice(), bank_slot);
        assert_eq!(
            result,
            Err(VerifySlotDeltasError::TooManyEntries(
                status_cache::MAX_CACHE_ENTRIES + 1,
                status_cache::MAX_CACHE_ENTRIES
            )),
        );
    }

    #[test]
    fn test_verify_slot_deltas_structural_good() {
        // NOTE: slot deltas do not need to be sorted
        let slot_deltas = vec![
            (222, true, Status::default()),
            (333, true, Status::default()),
            (111, true, Status::default()),
        ];

        let bank_slot = 333;
        let result = verify_slot_deltas_structural(slot_deltas.as_slice(), bank_slot);
        assert_eq!(
            result,
            Ok(VerifySlotDeltasStructuralInfo {
                slots: HashSet::from([111, 222, 333])
            })
        );
    }

    #[test]
    fn test_verify_slot_deltas_structural_bad_slot_not_root() {
        let slot_deltas = vec![
            (111, true, Status::default()),
            (222, false, Status::default()), // <-- slot is not a root
            (333, true, Status::default()),
        ];

        let bank_slot = 333;
        let result = verify_slot_deltas_structural(slot_deltas.as_slice(), bank_slot);
        assert_eq!(result, Err(VerifySlotDeltasError::SlotIsNotRoot(222)));
    }

    #[test]
    fn test_verify_slot_deltas_structural_bad_slot_greater_than_bank() {
        let slot_deltas = vec![
            (222, true, Status::default()),
            (111, true, Status::default()),
            (555, true, Status::default()), // <-- slot is greater than the bank slot
        ];

        let bank_slot = 444;
        let result = verify_slot_deltas_structural(slot_deltas.as_slice(), bank_slot);
        assert_eq!(
            result,
            Err(VerifySlotDeltasError::SlotGreaterThanMaxRoot(
                555, bank_slot
            )),
        );
    }

    #[test]
    fn test_verify_slot_deltas_structural_bad_slot_has_multiple_entries() {
        let slot_deltas = vec![
            (111, true, Status::default()),
            (222, true, Status::default()),
            (111, true, Status::default()), // <-- slot is a duplicate
        ];

        let bank_slot = 222;
        let result = verify_slot_deltas_structural(slot_deltas.as_slice(), bank_slot);
        assert_eq!(
            result,
            Err(VerifySlotDeltasError::SlotHasMultipleEntries(111)),
        );
    }

    #[test]
    fn test_verify_slot_deltas_with_history_good() {
        let mut slots_from_slot_deltas = HashSet::default();
        let mut slot_history = SlotHistory::default();
        // note: slot history expects slots to be added in numeric order
        for slot in [0, 111, 222, 333, 444] {
            slots_from_slot_deltas.insert(slot);
            slot_history.add(slot);
        }

        let bank_slot = 444;
        let result =
            verify_slot_deltas_with_history(&slots_from_slot_deltas, &slot_history, bank_slot);
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_verify_slot_deltas_with_history_bad_slot_history() {
        let bank_slot = 444;
        let result = verify_slot_deltas_with_history(
            &HashSet::default(),
            &SlotHistory::default(), // <-- will only have an entry for slot 0
            bank_slot,
        );
        assert_eq!(result, Err(VerifySlotDeltasError::BadSlotHistory));
    }

    #[test]
    fn test_verify_slot_deltas_with_history_bad_slot_not_in_history() {
        let slots_from_slot_deltas = HashSet::from([
            0, // slot history has slot 0 added by default
            444, 222,
        ]);
        let mut slot_history = SlotHistory::default();
        slot_history.add(444); // <-- slot history is missing slot 222

        let bank_slot = 444;
        let result =
            verify_slot_deltas_with_history(&slots_from_slot_deltas, &slot_history, bank_slot);

        assert_eq!(
            result,
            Err(VerifySlotDeltasError::SlotNotFoundInHistory(222)),
        );
    }

    #[test]
    fn test_verify_slot_deltas_with_history_bad_slot_not_in_deltas() {
        let slots_from_slot_deltas = HashSet::from([
            0, // slot history has slot 0 added by default
            444, 222,
            // <-- slot deltas is missing slot 333
        ]);
        let mut slot_history = SlotHistory::default();
        slot_history.add(222);
        slot_history.add(333);
        slot_history.add(444);

        let bank_slot = 444;
        let result =
            verify_slot_deltas_with_history(&slots_from_slot_deltas, &slot_history, bank_slot);

        assert_eq!(
            result,
            Err(VerifySlotDeltasError::SlotNotFoundInDeltas(333)),
        );
    }
}

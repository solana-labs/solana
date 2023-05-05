use {
    crate::{
        account_storage::AccountStorageMap,
        accounts_db::{
            AccountShrinkThreshold, AccountStorageEntry, AccountsDbConfig, AtomicAppendVecId,
            CalcAccountsHashDataSource,
        },
        accounts_hash::AccountsHash,
        accounts_index::AccountSecondaryIndexes,
        accounts_update_notifier_interface::AccountsUpdateNotifier,
        append_vec::AppendVec,
        bank::{Bank, BankFieldsToDeserialize, BankSlotDelta},
        builtins::Builtins,
        hardened_unpack::{
            streaming_unpack_snapshot, unpack_snapshot, ParallelSelector, UnpackError,
            UnpackedAppendVecMap,
        },
        runtime_config::RuntimeConfig,
        serde_snapshot::{
            bank_from_streams, bank_to_stream, fields_from_streams,
            BankIncrementalSnapshotPersistence, SerdeStyle, SnapshotStreams,
        },
        shared_buffer_reader::{SharedBuffer, SharedBufferReader},
        snapshot_archive_info::{
            FullSnapshotArchiveInfo, IncrementalSnapshotArchiveInfo, SnapshotArchiveInfoGetter,
        },
        snapshot_hash::SnapshotHash,
        snapshot_package::{AccountsPackage, AccountsPackageType, SnapshotPackage, SnapshotType},
        snapshot_utils::snapshot_storage_rebuilder::{
            RebuiltSnapshotStorage, SnapshotStorageRebuilder,
        },
        status_cache,
    },
    bincode::{config::Options, serialize_into},
    bzip2::bufread::BzDecoder,
    crossbeam_channel::Sender,
    flate2::read::GzDecoder,
    lazy_static::lazy_static,
    log::*,
    rayon::prelude::*,
    regex::Regex,
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
        cmp::Ordering,
        collections::{HashMap, HashSet},
        fmt,
        fs::{self, File},
        io::{BufReader, BufWriter, Error as IoError, ErrorKind, Read, Seek, Write},
        num::NonZeroUsize,
        path::{Path, PathBuf},
        process::ExitStatus,
        str::FromStr,
        sync::{
            atomic::{AtomicBool, AtomicU32},
            Arc,
        },
        thread::{Builder, JoinHandle},
    },
    tar::{self, Archive},
    tempfile::TempDir,
    thiserror::Error,
};

mod archive_format;
mod snapshot_storage_rebuilder;
pub use archive_format::*;

pub const SNAPSHOT_STATUS_CACHE_FILENAME: &str = "status_cache";
pub const SNAPSHOT_VERSION_FILENAME: &str = "version";
pub const SNAPSHOT_STATE_COMPLETE_FILENAME: &str = "state_complete";
pub const SNAPSHOT_ACCOUNTS_HARDLINKS: &str = "accounts_hardlinks";
pub const SNAPSHOT_ARCHIVE_DOWNLOAD_DIR: &str = "remote";
pub const DEFAULT_FULL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS: Slot = 25_000;
pub const DEFAULT_INCREMENTAL_SNAPSHOT_ARCHIVE_INTERVAL_SLOTS: Slot = 100;
const MAX_SNAPSHOT_DATA_FILE_SIZE: u64 = 32 * 1024 * 1024 * 1024; // 32 GiB
const MAX_SNAPSHOT_VERSION_FILE_SIZE: u64 = 8; // byte
const VERSION_STRING_V1_2_0: &str = "1.2.0";
pub const TMP_SNAPSHOT_ARCHIVE_PREFIX: &str = "tmp-snapshot-archive-";
pub const BANK_SNAPSHOT_PRE_FILENAME_EXTENSION: &str = "pre";
// Save some bank snapshots but not too many
pub const MAX_BANK_SNAPSHOTS_TO_RETAIN: usize = 8;
// The following unsafes are
// - Safe because the values are fixed, known non-zero constants
// - Necessary in order to have a plain NonZeroUsize as the constant, NonZeroUsize
//   returns an Option<NonZeroUsize> and we can't .unwrap() at compile time
pub const DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN: NonZeroUsize =
    unsafe { NonZeroUsize::new_unchecked(2) };
pub const DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN: NonZeroUsize =
    unsafe { NonZeroUsize::new_unchecked(4) };
pub const FULL_SNAPSHOT_ARCHIVE_FILENAME_REGEX: &str = r"^snapshot-(?P<slot>[[:digit:]]+)-(?P<hash>[[:alnum:]]+)\.(?P<ext>tar|tar\.bz2|tar\.zst|tar\.gz|tar\.lz4)$";
pub const INCREMENTAL_SNAPSHOT_ARCHIVE_FILENAME_REGEX: &str = r"^incremental-snapshot-(?P<base>[[:digit:]]+)-(?P<slot>[[:digit:]]+)-(?P<hash>[[:alnum:]]+)\.(?P<ext>tar|tar\.bz2|tar\.zst|tar\.gz|tar\.lz4)$";

#[derive(Copy, Clone, Default, Eq, PartialEq, Debug)]
pub enum SnapshotVersion {
    #[default]
    V1_2_0,
}

impl fmt::Display for SnapshotVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(From::from(*self))
    }
}

impl From<SnapshotVersion> for &'static str {
    fn from(snapshot_version: SnapshotVersion) -> &'static str {
        match snapshot_version {
            SnapshotVersion::V1_2_0 => VERSION_STRING_V1_2_0,
        }
    }
}

impl FromStr for SnapshotVersion {
    type Err = &'static str;

    fn from_str(version_string: &str) -> std::result::Result<Self, Self::Err> {
        // Remove leading 'v' or 'V' from slice
        let version_string = if version_string
            .get(..1)
            .map_or(false, |s| s.eq_ignore_ascii_case("v"))
        {
            &version_string[1..]
        } else {
            version_string
        };
        match version_string {
            VERSION_STRING_V1_2_0 => Ok(SnapshotVersion::V1_2_0),
            _ => Err("unsupported snapshot version"),
        }
    }
}

impl SnapshotVersion {
    pub fn as_str(self) -> &'static str {
        <&str as From<Self>>::from(self)
    }
}

/// Information about a bank snapshot. Namely the slot of the bank, the path to the snapshot, and
/// the type of the snapshot.
#[derive(PartialEq, Eq, Debug)]
pub struct BankSnapshotInfo {
    /// Slot of the bank
    pub slot: Slot,
    /// Type of the snapshot
    pub snapshot_type: BankSnapshotType,
    /// Path to the bank snapshot directory
    pub snapshot_dir: PathBuf,
    /// Snapshot version
    pub snapshot_version: SnapshotVersion,
}

impl PartialOrd for BankSnapshotInfo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Order BankSnapshotInfo by slot (ascending), which practically is sorting chronologically
impl Ord for BankSnapshotInfo {
    fn cmp(&self, other: &Self) -> Ordering {
        self.slot.cmp(&other.slot)
    }
}

impl BankSnapshotInfo {
    pub fn new_from_dir(
        bank_snapshots_dir: impl AsRef<Path>,
        slot: Slot,
    ) -> std::result::Result<BankSnapshotInfo, SnapshotNewFromDirError> {
        // check this directory to see if there is a BankSnapshotPre and/or
        // BankSnapshotPost file
        let bank_snapshot_dir = get_bank_snapshot_dir(&bank_snapshots_dir, slot);

        if !bank_snapshot_dir.is_dir() {
            return Err(SnapshotNewFromDirError::InvalidBankSnapshotDir(
                bank_snapshot_dir,
            ));
        }

        // Among the files checks, the completion flag file check should be done first to avoid the later
        // I/O errors.

        // There is a time window from the slot directory being created, and the content being completely
        // filled.  Check the completion to avoid using a highest found slot directory with missing content.
        let completion_flag_file = bank_snapshot_dir.join(SNAPSHOT_STATE_COMPLETE_FILENAME);
        if !completion_flag_file.is_file() {
            // If the directory is incomplete, it should be removed.
            // There are also possible hardlink files under <account_path>/snapshot/<slot>/, referred by this
            // snapshot dir's symlinks.  They are cleaned up in clean_orphaned_account_snapshot_dirs() at the
            // boot time.
            info!("Removing incomplete snapshot dir: {:?}", bank_snapshot_dir);
            fs::remove_dir_all(&bank_snapshot_dir)?;
            return Err(SnapshotNewFromDirError::IncompleteDir(bank_snapshot_dir));
        }

        let status_cache_file = bank_snapshot_dir.join(SNAPSHOT_STATUS_CACHE_FILENAME);
        if !status_cache_file.is_file() {
            return Err(SnapshotNewFromDirError::MissingStatusCacheFile(
                status_cache_file,
            ));
        }

        let version_path = bank_snapshot_dir.join(SNAPSHOT_VERSION_FILENAME);
        let version_str = snapshot_version_from_file(&version_path).or(Err(
            SnapshotNewFromDirError::MissingVersionFile(version_path),
        ))?;
        let snapshot_version = SnapshotVersion::from_str(version_str.as_str())
            .or(Err(SnapshotNewFromDirError::InvalidVersion))?;

        let bank_snapshot_post_path = bank_snapshot_dir.join(get_snapshot_file_name(slot));
        let bank_snapshot_pre_path =
            bank_snapshot_post_path.with_extension(BANK_SNAPSHOT_PRE_FILENAME_EXTENSION);

        let snapshot_type = if bank_snapshot_pre_path.is_file() {
            BankSnapshotType::Pre
        } else if bank_snapshot_post_path.is_file() {
            BankSnapshotType::Post
        } else {
            return Err(SnapshotNewFromDirError::MissingSnapshotFile(
                bank_snapshot_dir,
            ));
        };

        Ok(BankSnapshotInfo {
            slot,
            snapshot_type,
            snapshot_dir: bank_snapshot_dir,
            snapshot_version,
        })
    }

    pub fn snapshot_path(&self) -> PathBuf {
        let mut bank_snapshot_path = self.snapshot_dir.join(get_snapshot_file_name(self.slot));

        let ext = match self.snapshot_type {
            BankSnapshotType::Pre => BANK_SNAPSHOT_PRE_FILENAME_EXTENSION,
            BankSnapshotType::Post => "",
        };
        bank_snapshot_path.set_extension(ext);

        bank_snapshot_path
    }
}
/// Bank snapshots traditionally had their accounts hash calculated prior to serialization.  Since
/// the hash calculation takes a long time, an optimization has been put in to offload the accounts
/// hash calculation.  The bank serialization format has not changed, so we need another way to
/// identify if a bank snapshot contains the calculated accounts hash or not.
///
/// When a bank snapshot is first taken, it does not have the calculated accounts hash.  It is said
/// that this bank snapshot is "pre" accounts hash.  Later, when the accounts hash is calculated,
/// the bank snapshot is re-serialized, and is now "post" accounts hash.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BankSnapshotType {
    /// This bank snapshot has *not* yet had its accounts hash calculated
    Pre,
    /// This bank snapshot *has* had its accounts hash calculated
    Post,
}

/// When constructing a bank a snapshot, traditionally the snapshot was from a snapshot archive.  Now,
/// the snapshot can be from a snapshot directory, or from a snapshot archive.  This is the flag to
/// indicate which.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotFrom {
    /// Build from the snapshot archive
    Archive,
    /// Build directly from the bank snapshot directory
    Dir,
}

/// Helper type when rebuilding from snapshots.  Designed to handle when rebuilding from just a
/// full snapshot, or from both a full snapshot and an incremental snapshot.
#[derive(Debug)]
struct SnapshotRootPaths {
    full_snapshot_root_file_path: PathBuf,
    incremental_snapshot_root_file_path: Option<PathBuf>,
}

/// Helper type to bundle up the results from `unarchive_snapshot()`
#[derive(Debug)]
struct UnarchivedSnapshot {
    #[allow(dead_code)]
    unpack_dir: TempDir,
    storage: AccountStorageMap,
    unpacked_snapshots_dir_and_version: UnpackedSnapshotsDirAndVersion,
    measure_untar: Measure,
}

/// Helper type for passing around the unpacked snapshots dir and the snapshot version together
#[derive(Debug)]
struct UnpackedSnapshotsDirAndVersion {
    unpacked_snapshots_dir: PathBuf,
    snapshot_version: SnapshotVersion,
}

/// Helper type for passing around account storage map and next append vec id
/// for reconstructing accounts from a snapshot
pub(crate) struct StorageAndNextAppendVecId {
    pub storage: AccountStorageMap,
    pub next_append_vec_id: AtomicAppendVecId,
}

#[derive(Error, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum SnapshotError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialize(#[from] bincode::Error),

    #[error("crossbeam send error: {0}")]
    CrossbeamSend(#[from] crossbeam_channel::SendError<PathBuf>),

    #[error("archive generation failure {0}")]
    ArchiveGenerationFailure(ExitStatus),

    #[error("storage path symlink is invalid")]
    StoragePathSymlinkInvalid,

    #[error("Unpack error: {0}")]
    UnpackError(#[from] UnpackError),

    #[error("source({1}) - I/O error: {0}")]
    IoWithSource(std::io::Error, &'static str),

    #[error("source({1}) - I/O error: {0}, file: {2}")]
    IoWithSourceAndFile(#[source] std::io::Error, &'static str, PathBuf),

    #[error("could not get file name from path: {}", .0.display())]
    PathToFileNameError(PathBuf),

    #[error("could not get str from file name: {}", .0.display())]
    FileNameToStrError(PathBuf),

    #[error("could not parse snapshot archive's file name: {0}")]
    ParseSnapshotArchiveFileNameError(String),

    #[error("snapshots are incompatible: full snapshot slot ({0}) and incremental snapshot base slot ({1}) do not match")]
    MismatchedBaseSlot(Slot, Slot),

    #[error("no snapshot archives to load from")]
    NoSnapshotArchives,

    #[error("snapshot has mismatch: deserialized bank: {:?}, snapshot archive info: {:?}", .0, .1)]
    MismatchedSlotHash((Slot, SnapshotHash), (Slot, SnapshotHash)),

    #[error("snapshot slot deltas are invalid: {0}")]
    VerifySlotDeltas(#[from] VerifySlotDeltasError),

    #[error("bank_snapshot_info new_from_dir failed: {0}")]
    NewFromDir(#[from] SnapshotNewFromDirError),

    #[error("invalid snapshot dir path: {}", .0.display())]
    InvalidSnapshotDirPath(PathBuf),

    #[error("invalid AppendVec path: {}", .0.display())]
    InvalidAppendVecPath(PathBuf),

    #[error("invalid account path: {}", .0.display())]
    InvalidAccountPath(PathBuf),

    #[error("no valid snapshot dir found under {}", .0.display())]
    NoSnapshotSlotDir(PathBuf),

    #[error("snapshot dir account paths mismatching")]
    AccountPathsMismatch,
}

#[derive(Error, Debug)]
pub enum SnapshotNewFromDirError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid bank snapshot directory {}", .0.display())]
    InvalidBankSnapshotDir(PathBuf),

    #[error("missing status cache file {}", .0.display())]
    MissingStatusCacheFile(PathBuf),

    #[error("missing version file {}", .0.display())]
    MissingVersionFile(PathBuf),

    #[error("invalid snapshot version")]
    InvalidVersion,

    #[error("snapshot directory incomplete {}", .0.display())]
    IncompleteDir(PathBuf),

    #[error("missing snapshot file {}", .0.display())]
    MissingSnapshotFile(PathBuf),
}

pub type Result<T> = std::result::Result<T, SnapshotError>;

/// Errors that can happen in `verify_slot_deltas()`
#[derive(Error, Debug, PartialEq, Eq)]
pub enum VerifySlotDeltasError {
    #[error("too many entries: {0} (max: {1})")]
    TooManyEntries(usize, usize),

    #[error("slot {0} is not a root")]
    SlotIsNotRoot(Slot),

    #[error("slot {0} is greater than bank slot {1}")]
    SlotGreaterThanMaxRoot(Slot, Slot),

    #[error("slot {0} has multiple entries")]
    SlotHasMultipleEntries(Slot),

    #[error("slot {0} was not found in slot history")]
    SlotNotFoundInHistory(Slot),

    #[error("slot {0} was in history but missing from slot deltas")]
    SlotNotFoundInDeltas(Slot),

    #[error("slot history is bad and cannot be used to verify slot deltas")]
    BadSlotHistory,
}

/// Delete the files and subdirectories in a directory.
/// This is useful if the process does not have permission
/// to delete the top level directory it might be able to
/// delete the contents of that directory.
fn delete_contents_of_path(path: impl AsRef<Path>) {
    if let Ok(dir_entries) = std::fs::read_dir(&path) {
        for entry in dir_entries.flatten() {
            let sub_path = entry.path();
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(err) => {
                    warn!(
                        "Failed to get metadata for {}. Error: {}",
                        sub_path.display(),
                        err.to_string()
                    );
                    break;
                }
            };
            if metadata.is_dir() {
                if let Err(err) = std::fs::remove_dir_all(&sub_path) {
                    warn!(
                        "Failed to remove sub directory {}.  Error: {}",
                        sub_path.display(),
                        err.to_string()
                    );
                }
            } else if metadata.is_file() {
                if let Err(err) = std::fs::remove_file(&sub_path) {
                    warn!(
                        "Failed to remove file {}.  Error: {}",
                        sub_path.display(),
                        err.to_string()
                    );
                }
            }
        }
    } else {
        warn!(
            "Failed to read the sub paths of {}",
            path.as_ref().display()
        );
    }
}

/// Delete directories/files asynchronously to avoid blocking on it.
/// Fist, in sync context, rename the original path to *_deleted,
/// then spawn a thread to delete the renamed path.
/// If the process is killed and the deleting process is not done,
/// the leftover path will be deleted in the next process life, so
/// there is no file space leaking.
pub fn move_and_async_delete_path(path: impl AsRef<Path>) {
    let mut path_delete = PathBuf::new();
    path_delete.push(&path);
    path_delete.set_file_name(format!(
        "{}{}",
        path_delete.file_name().unwrap().to_str().unwrap(),
        "_to_be_deleted"
    ));

    if path_delete.exists() {
        std::fs::remove_dir_all(&path_delete).unwrap();
    }

    if !path.as_ref().exists() {
        return;
    }

    if let Err(err) = std::fs::rename(&path, &path_delete) {
        warn!(
            "Path renaming failed: {}.  Falling back to rm_dir in sync mode",
            err.to_string()
        );
        delete_contents_of_path(path);
        return;
    }

    Builder::new()
        .name("solDeletePath".to_string())
        .spawn(move || {
            std::fs::remove_dir_all(path_delete).unwrap();
        })
        .unwrap();
}

/// The account snapshot directories under <account_path>/snapshot/<slot> contain account files hardlinked
/// from <account_path>/run taken at snapshot <slot> time.  They are referenced by the symlinks from the
/// bank snapshot dir snapshot/<slot>/accounts_hardlinks/.  We observed that sometimes the bank snapshot dir
/// could be deleted but the account snapshot directories were left behind, possibly by some manual operations
/// or some legacy code not using the symlinks to clean up the acccount snapshot hardlink directories.
/// This function cleans up any account snapshot directories that are no longer referenced by the bank
/// snapshot dirs, to ensure proper snapshot operations.
pub fn clean_orphaned_account_snapshot_dirs(
    bank_snapshots_dir: impl AsRef<Path>,
    account_snapshot_paths: &[PathBuf],
) -> Result<()> {
    // Create the HashSet of the account snapshot hardlink directories referenced by the snapshot dirs.
    // This is used to clean up any hardlinks that are no longer referenced by the snapshot dirs.
    let mut account_snapshot_dirs_referenced = HashSet::new();
    let snapshots = get_bank_snapshots(bank_snapshots_dir);
    for snapshot in snapshots {
        let account_hardlinks_dir = snapshot.snapshot_dir.join(SNAPSHOT_ACCOUNTS_HARDLINKS);
        // loop through entries in the snapshot_hardlink_dir, read the symlinks, add the target to the HashSet
        for entry in fs::read_dir(&account_hardlinks_dir)? {
            let path = entry?.path();
            let target = fs::read_link(&path)?;
            account_snapshot_dirs_referenced.insert(target);
        }
    }

    // loop through the account snapshot hardlink directories, if the directory is not in the account_snapshot_dirs_referenced set, delete it
    for account_snapshot_path in account_snapshot_paths {
        for entry in fs::read_dir(account_snapshot_path)? {
            let path = entry?.path();
            if !account_snapshot_dirs_referenced.contains(&path) {
                info!(
                    "Removing orphaned account snapshot hardlink directory: {}",
                    path.display()
                );
                move_and_async_delete_path(&path);
            }
        }
    }

    Ok(())
}

/// If the validator halts in the middle of `archive_snapshot_package()`, the temporary staging
/// directory won't be cleaned up.  Call this function to clean them up.
pub fn remove_tmp_snapshot_archives(snapshot_archives_dir: impl AsRef<Path>) {
    if let Ok(entries) = fs::read_dir(snapshot_archives_dir) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            let file_name = entry
                .file_name()
                .into_string()
                .unwrap_or_else(|_| String::new());
            if file_name.starts_with(TMP_SNAPSHOT_ARCHIVE_PREFIX) {
                if entry.path().is_file() {
                    fs::remove_file(entry.path())
                } else {
                    fs::remove_dir_all(entry.path())
                }
                .unwrap_or_else(|err| {
                    warn!("Failed to remove {}: {}", entry.path().display(), err)
                });
            }
        }
    }
}

/// Write the snapshot version as a file into the bank snapshot directory
pub fn write_snapshot_version_file(
    version_file: impl AsRef<Path>,
    version: SnapshotVersion,
) -> Result<()> {
    let mut f = fs::File::create(version_file)
        .map_err(|e| SnapshotError::IoWithSource(e, "create version file"))?;
    f.write_all(version.as_str().as_bytes())
        .map_err(|e| SnapshotError::IoWithSource(e, "write version file"))?;
    Ok(())
}

/// Make a snapshot archive out of the snapshot package
pub fn archive_snapshot_package(
    snapshot_package: &SnapshotPackage,
    full_snapshot_archives_dir: impl AsRef<Path>,
    incremental_snapshot_archives_dir: impl AsRef<Path>,
    maximum_full_snapshot_archives_to_retain: NonZeroUsize,
    maximum_incremental_snapshot_archives_to_retain: NonZeroUsize,
) -> Result<()> {
    info!(
        "Generating snapshot archive for slot {}",
        snapshot_package.slot()
    );

    let mut timer = Measure::start("snapshot_package-package_snapshots");
    let tar_dir = snapshot_package
        .path()
        .parent()
        .expect("Tar output path is invalid");

    fs::create_dir_all(tar_dir).map_err(|e| {
        SnapshotError::IoWithSourceAndFile(e, "create archive path", tar_dir.into())
    })?;

    // Create the staging directories
    let staging_dir_prefix = TMP_SNAPSHOT_ARCHIVE_PREFIX;
    let staging_dir = tempfile::Builder::new()
        .prefix(&format!(
            "{}{}-",
            staging_dir_prefix,
            snapshot_package.slot()
        ))
        .tempdir_in(tar_dir)
        .map_err(|e| SnapshotError::IoWithSource(e, "create archive tempdir"))?;

    let staging_accounts_dir = staging_dir.path().join("accounts");
    let staging_snapshots_dir = staging_dir.path().join("snapshots");
    let staging_version_file = staging_dir.path().join(SNAPSHOT_VERSION_FILENAME);

    // Create staging/accounts/
    fs::create_dir_all(&staging_accounts_dir).map_err(|e| {
        SnapshotError::IoWithSourceAndFile(
            e,
            "create staging accounts path",
            staging_accounts_dir.clone(),
        )
    })?;

    let slot_str = snapshot_package.slot().to_string();
    let staging_snapshot_dir = staging_snapshots_dir.join(&slot_str);
    // Creates staging snapshots/<slot>/
    fs::create_dir_all(&staging_snapshot_dir).map_err(|e| {
        SnapshotError::IoWithSourceAndFile(
            e,
            "create staging snapshots path",
            staging_snapshots_dir.clone(),
        )
    })?;

    let src_snapshot_dir = &snapshot_package.bank_snapshot_dir;
    // To be a source for symlinking and archiving, the path need to be an aboslute path
    let src_snapshot_dir = src_snapshot_dir
        .canonicalize()
        .map_err(|_e| SnapshotError::InvalidSnapshotDirPath(src_snapshot_dir.clone()))?;
    let staging_snapshot_file = staging_snapshot_dir.join(&slot_str);
    let src_snapshot_file = src_snapshot_dir.join(slot_str);
    symlink::symlink_file(src_snapshot_file, staging_snapshot_file)
        .map_err(|e| SnapshotError::IoWithSource(e, "create snapshot symlink"))?;

    // Following the existing archive format, the status cache is under snapshots/, not under <slot>/
    // like in the snapshot dir.
    let staging_status_cache = staging_snapshots_dir.join(SNAPSHOT_STATUS_CACHE_FILENAME);
    let src_status_cache = src_snapshot_dir.join(SNAPSHOT_STATUS_CACHE_FILENAME);
    symlink::symlink_file(src_status_cache, staging_status_cache)
        .map_err(|e| SnapshotError::IoWithSource(e, "create status cache symlink"))?;

    // Add the AppendVecs into the compressible list
    for storage in snapshot_package.snapshot_storages.iter() {
        storage.flush()?;
        let storage_path = storage.get_path();
        let output_path = staging_accounts_dir.join(crate::append_vec::AppendVec::file_name(
            storage.slot(),
            storage.append_vec_id(),
        ));

        // `storage_path` - The file path where the AppendVec itself is located
        // `output_path` - The file path where the AppendVec will be placed in the staging directory.
        let storage_path =
            fs::canonicalize(storage_path).expect("Could not get absolute path for accounts");
        symlink::symlink_file(storage_path, &output_path)
            .map_err(|e| SnapshotError::IoWithSource(e, "create storage symlink"))?;
        if !output_path.is_file() {
            return Err(SnapshotError::StoragePathSymlinkInvalid);
        }
    }

    write_snapshot_version_file(staging_version_file, snapshot_package.snapshot_version)?;

    // Tar the staging directory into the archive at `archive_path`
    let archive_path = tar_dir.join(format!(
        "{}{}.{}",
        staging_dir_prefix,
        snapshot_package.slot(),
        snapshot_package.archive_format().extension(),
    ));

    {
        let mut archive_file = fs::File::create(&archive_path)?;

        let do_archive_files = |encoder: &mut dyn Write| -> Result<()> {
            let mut archive = tar::Builder::new(encoder);
            // Serialize the version and snapshots files before accounts so we can quickly determine the version
            // and other bank fields. This is necessary if we want to interleave unpacking with reconstruction
            archive.append_path_with_name(
                staging_dir.as_ref().join(SNAPSHOT_VERSION_FILENAME),
                SNAPSHOT_VERSION_FILENAME,
            )?;
            for dir in ["snapshots", "accounts"] {
                archive.append_dir_all(dir, staging_dir.as_ref().join(dir))?;
            }
            archive.into_inner()?;
            Ok(())
        };

        match snapshot_package.archive_format() {
            ArchiveFormat::TarBzip2 => {
                let mut encoder =
                    bzip2::write::BzEncoder::new(archive_file, bzip2::Compression::best());
                do_archive_files(&mut encoder)?;
                encoder.finish()?;
            }
            ArchiveFormat::TarGzip => {
                let mut encoder =
                    flate2::write::GzEncoder::new(archive_file, flate2::Compression::default());
                do_archive_files(&mut encoder)?;
                encoder.finish()?;
            }
            ArchiveFormat::TarZstd => {
                let mut encoder = zstd::stream::Encoder::new(archive_file, 0)?;
                do_archive_files(&mut encoder)?;
                encoder.finish()?;
            }
            ArchiveFormat::TarLz4 => {
                let mut encoder = lz4::EncoderBuilder::new().level(1).build(archive_file)?;
                do_archive_files(&mut encoder)?;
                let (_output, result) = encoder.finish();
                result?
            }
            ArchiveFormat::Tar => {
                do_archive_files(&mut archive_file)?;
            }
        };
    }

    // Atomically move the archive into position for other validators to find
    let metadata = fs::metadata(&archive_path).map_err(|e| {
        SnapshotError::IoWithSourceAndFile(e, "archive path stat", archive_path.clone())
    })?;
    fs::rename(&archive_path, snapshot_package.path())
        .map_err(|e| SnapshotError::IoWithSource(e, "archive path rename"))?;

    purge_old_snapshot_archives(
        full_snapshot_archives_dir,
        incremental_snapshot_archives_dir,
        maximum_full_snapshot_archives_to_retain,
        maximum_incremental_snapshot_archives_to_retain,
    );

    timer.stop();
    info!(
        "Successfully created {:?}. slot: {}, elapsed ms: {}, size={}",
        snapshot_package.path(),
        snapshot_package.slot(),
        timer.as_ms(),
        metadata.len()
    );

    datapoint_info!(
        "archive-snapshot-package",
        ("slot", snapshot_package.slot(), i64),
        (
            "archive_format",
            snapshot_package.archive_format().to_string(),
            String
        ),
        ("duration_ms", timer.as_ms(), i64),
        (
            if snapshot_package.snapshot_type.is_full_snapshot() {
                "full-snapshot-archive-size"
            } else {
                "incremental-snapshot-archive-size"
            },
            metadata.len(),
            i64
        ),
    );
    Ok(())
}

/// Get the bank snapshots in a directory
pub fn get_bank_snapshots(bank_snapshots_dir: impl AsRef<Path>) -> Vec<BankSnapshotInfo> {
    let mut bank_snapshots = Vec::default();
    match fs::read_dir(&bank_snapshots_dir) {
        Err(err) => {
            info!(
                "Unable to read bank snapshots directory {}: {}",
                bank_snapshots_dir.as_ref().display(),
                err
            );
        }
        Ok(paths) => paths
            .filter_map(|entry| {
                // check if this entry is a directory and only a Slot
                // bank snapshots are bank_snapshots_dir/slot/slot(BANK_SNAPSHOT_PRE_FILENAME_EXTENSION)
                entry
                    .ok()
                    .filter(|entry| entry.path().is_dir())
                    .and_then(|entry| {
                        entry
                            .path()
                            .file_name()
                            .and_then(|file_name| file_name.to_str())
                            .and_then(|file_name| file_name.parse::<Slot>().ok())
                    })
            })
            .for_each(
                |slot| match BankSnapshotInfo::new_from_dir(&bank_snapshots_dir, slot) {
                    Ok(snapshot_info) => {
                        bank_snapshots.push(snapshot_info);
                    }
                    Err(err) => {
                        error!("Unable to read bank snapshot for slot {}: {}", slot, err);
                    }
                },
            ),
    }
    bank_snapshots
}

/// Get the bank snapshots in a directory
///
/// This function retains only the bank snapshots of type BankSnapshotType::Pre
pub fn get_bank_snapshots_pre(bank_snapshots_dir: impl AsRef<Path>) -> Vec<BankSnapshotInfo> {
    let mut bank_snapshots = get_bank_snapshots(bank_snapshots_dir);
    bank_snapshots.retain(|bank_snapshot| bank_snapshot.snapshot_type == BankSnapshotType::Pre);
    bank_snapshots
}

/// Get the bank snapshots in a directory
///
/// This function retains only the bank snapshots of type BankSnapshotType::Post
pub fn get_bank_snapshots_post(bank_snapshots_dir: impl AsRef<Path>) -> Vec<BankSnapshotInfo> {
    let mut bank_snapshots = get_bank_snapshots(bank_snapshots_dir);
    bank_snapshots.retain(|bank_snapshot| bank_snapshot.snapshot_type == BankSnapshotType::Post);
    bank_snapshots
}

/// Get the bank snapshot with the highest slot in a directory
///
/// This function gets the highest bank snapshot of type BankSnapshotType::Pre
pub fn get_highest_bank_snapshot_pre(
    bank_snapshots_dir: impl AsRef<Path>,
) -> Option<BankSnapshotInfo> {
    do_get_highest_bank_snapshot(get_bank_snapshots_pre(bank_snapshots_dir))
}

/// Get the bank snapshot with the highest slot in a directory
///
/// This function gets the highest bank snapshot of type BankSnapshotType::Post
pub fn get_highest_bank_snapshot_post(
    bank_snapshots_dir: impl AsRef<Path>,
) -> Option<BankSnapshotInfo> {
    do_get_highest_bank_snapshot(get_bank_snapshots_post(bank_snapshots_dir))
}

/// Get the bank snapshot with the highest slot in a directory
///
/// This function gets the highest bank snapshot of any type
pub fn get_highest_bank_snapshot(bank_snapshots_dir: impl AsRef<Path>) -> Option<BankSnapshotInfo> {
    do_get_highest_bank_snapshot(get_bank_snapshots(&bank_snapshots_dir))
}

fn do_get_highest_bank_snapshot(
    mut bank_snapshots: Vec<BankSnapshotInfo>,
) -> Option<BankSnapshotInfo> {
    bank_snapshots.sort_unstable();
    bank_snapshots.into_iter().rev().next()
}

pub fn serialize_snapshot_data_file<F>(data_file_path: &Path, serializer: F) -> Result<u64>
where
    F: FnOnce(&mut BufWriter<File>) -> Result<()>,
{
    serialize_snapshot_data_file_capped::<F>(
        data_file_path,
        MAX_SNAPSHOT_DATA_FILE_SIZE,
        serializer,
    )
}

pub fn deserialize_snapshot_data_file<T: Sized>(
    data_file_path: &Path,
    deserializer: impl FnOnce(&mut BufReader<File>) -> Result<T>,
) -> Result<T> {
    let wrapped_deserializer = move |streams: &mut SnapshotStreams<File>| -> Result<T> {
        deserializer(streams.full_snapshot_stream)
    };

    let wrapped_data_file_path = SnapshotRootPaths {
        full_snapshot_root_file_path: data_file_path.to_path_buf(),
        incremental_snapshot_root_file_path: None,
    };

    deserialize_snapshot_data_files_capped(
        &wrapped_data_file_path,
        MAX_SNAPSHOT_DATA_FILE_SIZE,
        wrapped_deserializer,
    )
}

fn deserialize_snapshot_data_files<T: Sized>(
    snapshot_root_paths: &SnapshotRootPaths,
    deserializer: impl FnOnce(&mut SnapshotStreams<File>) -> Result<T>,
) -> Result<T> {
    deserialize_snapshot_data_files_capped(
        snapshot_root_paths,
        MAX_SNAPSHOT_DATA_FILE_SIZE,
        deserializer,
    )
}

fn serialize_snapshot_data_file_capped<F>(
    data_file_path: &Path,
    maximum_file_size: u64,
    serializer: F,
) -> Result<u64>
where
    F: FnOnce(&mut BufWriter<File>) -> Result<()>,
{
    let data_file = File::create(data_file_path)?;
    let mut data_file_stream = BufWriter::new(data_file);
    serializer(&mut data_file_stream)?;
    data_file_stream.flush()?;

    let consumed_size = data_file_stream.stream_position()?;
    if consumed_size > maximum_file_size {
        let error_message = format!(
            "too large snapshot data file to serialize: {data_file_path:?} has {consumed_size} bytes"
        );
        return Err(get_io_error(&error_message));
    }
    Ok(consumed_size)
}

fn deserialize_snapshot_data_files_capped<T: Sized>(
    snapshot_root_paths: &SnapshotRootPaths,
    maximum_file_size: u64,
    deserializer: impl FnOnce(&mut SnapshotStreams<File>) -> Result<T>,
) -> Result<T> {
    let (full_snapshot_file_size, mut full_snapshot_data_file_stream) =
        create_snapshot_data_file_stream(
            &snapshot_root_paths.full_snapshot_root_file_path,
            maximum_file_size,
        )?;

    let (incremental_snapshot_file_size, mut incremental_snapshot_data_file_stream) =
        if let Some(ref incremental_snapshot_root_file_path) =
            snapshot_root_paths.incremental_snapshot_root_file_path
        {
            let (incremental_snapshot_file_size, incremental_snapshot_data_file_stream) =
                create_snapshot_data_file_stream(
                    incremental_snapshot_root_file_path,
                    maximum_file_size,
                )?;
            (
                Some(incremental_snapshot_file_size),
                Some(incremental_snapshot_data_file_stream),
            )
        } else {
            (None, None)
        };

    let mut snapshot_streams = SnapshotStreams {
        full_snapshot_stream: &mut full_snapshot_data_file_stream,
        incremental_snapshot_stream: incremental_snapshot_data_file_stream.as_mut(),
    };
    let ret = deserializer(&mut snapshot_streams)?;

    check_deserialize_file_consumed(
        full_snapshot_file_size,
        &snapshot_root_paths.full_snapshot_root_file_path,
        &mut full_snapshot_data_file_stream,
    )?;

    if let Some(ref incremental_snapshot_root_file_path) =
        snapshot_root_paths.incremental_snapshot_root_file_path
    {
        check_deserialize_file_consumed(
            incremental_snapshot_file_size.unwrap(),
            incremental_snapshot_root_file_path,
            incremental_snapshot_data_file_stream.as_mut().unwrap(),
        )?;
    }

    Ok(ret)
}

/// Before running the deserializer function, perform common operations on the snapshot archive
/// files, such as checking the file size and opening the file into a stream.
fn create_snapshot_data_file_stream(
    snapshot_root_file_path: impl AsRef<Path>,
    maximum_file_size: u64,
) -> Result<(u64, BufReader<File>)> {
    let snapshot_file_size = fs::metadata(&snapshot_root_file_path)?.len();

    if snapshot_file_size > maximum_file_size {
        let error_message =
            format!(
            "too large snapshot data file to deserialize: {} has {} bytes (max size is {} bytes)",
            snapshot_root_file_path.as_ref().display(), snapshot_file_size, maximum_file_size
        );
        return Err(get_io_error(&error_message));
    }

    let snapshot_data_file = File::open(&snapshot_root_file_path)?;
    let snapshot_data_file_stream = BufReader::new(snapshot_data_file);

    Ok((snapshot_file_size, snapshot_data_file_stream))
}

/// After running the deserializer function, perform common checks to ensure the snapshot archive
/// files were consumed correctly.
fn check_deserialize_file_consumed(
    file_size: u64,
    file_path: impl AsRef<Path>,
    file_stream: &mut BufReader<File>,
) -> Result<()> {
    let consumed_size = file_stream.stream_position()?;

    if consumed_size != file_size {
        let error_message =
            format!(
            "invalid snapshot data file: {} has {} bytes, however consumed {} bytes to deserialize",
            file_path.as_ref().display(), file_size, consumed_size
        );
        return Err(get_io_error(&error_message));
    }

    Ok(())
}

/// To allow generating a bank snapshot directory with full state information, we need to
/// hardlink account appendvec files from the runtime operation directory to a snapshot
/// hardlink directory.  This is to create the run/ and snapshot sub directories for an
/// account_path provided by the user.  These two sub directories are on the same file
/// system partition to allow hard-linking.
pub fn create_accounts_run_and_snapshot_dirs(
    account_dir: impl AsRef<Path>,
) -> std::io::Result<(PathBuf, PathBuf)> {
    let run_path = account_dir.as_ref().join("run");
    let snapshot_path = account_dir.as_ref().join("snapshot");
    if (!run_path.is_dir()) || (!snapshot_path.is_dir()) {
        // If the "run/" or "snapshot" sub directories do not exist, the directory may be from
        // an older version for which the appendvec files are at this directory.  Clean up
        // them first.
        // This will be done only once when transitioning from an old image without run directory
        // to this new version using run and snapshot directories.
        // The run/ content cleanup will be done at a later point.  The snapshot/ content persists
        // across the process boot, and will be purged by the account_background_service.
        if fs::remove_dir_all(&account_dir).is_err() {
            delete_contents_of_path(&account_dir);
        }
        fs::create_dir_all(&run_path)?;
        fs::create_dir_all(&snapshot_path)?;
    }

    Ok((run_path, snapshot_path))
}

/// For all account_paths, create the run/ and snapshot/ sub directories.
/// If an account_path directory does not exist, create it.
/// It returns (account_run_paths, account_snapshot_paths) or error
pub fn create_all_accounts_run_and_snapshot_dirs(
    account_paths: &[PathBuf],
) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut run_dirs = Vec::with_capacity(account_paths.len());
    let mut snapshot_dirs = Vec::with_capacity(account_paths.len());
    for account_path in account_paths {
        // create the run/ and snapshot/ sub directories for each account_path
        let (run_dir, snapshot_dir) =
            create_accounts_run_and_snapshot_dirs(account_path).map_err(|err| {
                SnapshotError::IoWithSourceAndFile(
                    err,
                    "Unable to create account run and snapshot directories",
                    account_path.to_path_buf(),
                )
            })?;
        run_dirs.push(run_dir);
        snapshot_dirs.push(snapshot_dir);
    }
    Ok((run_dirs, snapshot_dirs))
}

/// Return account path from the appendvec path after checking its format.
fn get_account_path_from_appendvec_path(appendvec_path: &Path) -> Option<PathBuf> {
    let run_path = appendvec_path.parent()?;
    let run_file_name = run_path.file_name()?;
    // All appendvec files should be under <account_path>/run/.
    // When generating the bank snapshot directory, they are hardlinked to <account_path>/snapshot/<slot>/
    if run_file_name != "run" {
        error!(
            "The account path {} does not have run/ as its immediate parent directory.",
            run_path.display()
        );
        return None;
    }
    let account_path = run_path.parent()?;
    Some(account_path.to_path_buf())
}

/// From an appendvec path, derive the snapshot hardlink path.  If the corresponding snapshot hardlink
/// directory does not exist, create it.
fn get_snapshot_accounts_hardlink_dir(
    appendvec_path: &Path,
    bank_slot: Slot,
    account_paths: &mut HashSet<PathBuf>,
    hardlinks_dir: impl AsRef<Path>,
) -> Result<PathBuf> {
    let account_path = get_account_path_from_appendvec_path(appendvec_path)
        .ok_or_else(|| SnapshotError::InvalidAppendVecPath(appendvec_path.to_path_buf()))?;

    let snapshot_hardlink_dir = account_path.join("snapshot").join(bank_slot.to_string());

    // Use the hashset to track, to avoid checking the file system.  Only set up the hardlink directory
    // and the symlink to it at the first time of seeing the account_path.
    if !account_paths.contains(&account_path) {
        let idx = account_paths.len();
        debug!(
            "for appendvec_path {}, create hard-link path {}",
            appendvec_path.display(),
            snapshot_hardlink_dir.display()
        );
        fs::create_dir_all(&snapshot_hardlink_dir).map_err(|e| {
            SnapshotError::IoWithSourceAndFile(
                e,
                "create hard-link dir",
                snapshot_hardlink_dir.clone(),
            )
        })?;
        let symlink_path = hardlinks_dir.as_ref().join(format!("account_path_{idx}"));
        symlink::symlink_dir(&snapshot_hardlink_dir, symlink_path).map_err(|e| {
            SnapshotError::IoWithSourceAndFile(
                e,
                "simlink the hard-link dir",
                snapshot_hardlink_dir.clone(),
            )
        })?;
        account_paths.insert(account_path);
    };

    Ok(snapshot_hardlink_dir)
}

/// Hard-link the files from accounts/ to snapshot/<bank_slot>/accounts/
/// This keeps the appendvec files alive and with the bank snapshot.  The slot and id
/// in the file names are also updated in case its file is a recycled one with inconsistent slot
/// and id.
fn hard_link_storages_to_snapshot(
    bank_snapshot_dir: impl AsRef<Path>,
    bank_slot: Slot,
    snapshot_storages: &[Arc<AccountStorageEntry>],
) -> Result<()> {
    let accounts_hardlinks_dir = bank_snapshot_dir.as_ref().join(SNAPSHOT_ACCOUNTS_HARDLINKS);
    fs::create_dir_all(&accounts_hardlinks_dir)?;

    let mut account_paths: HashSet<PathBuf> = HashSet::new();
    for storage in snapshot_storages {
        storage.flush()?;
        let storage_path = storage.accounts.get_path();
        let snapshot_hardlink_dir = get_snapshot_accounts_hardlink_dir(
            &storage_path,
            bank_slot,
            &mut account_paths,
            &accounts_hardlinks_dir,
        )?;
        // The appendvec could be recycled, so its filename may not be consistent to the slot and id.
        // Use the storage slot and id to compose a consistent file name for the hard-link file.
        let hardlink_filename = AppendVec::file_name(storage.slot(), storage.append_vec_id());
        let hard_link_path = snapshot_hardlink_dir.join(hardlink_filename);
        fs::hard_link(&storage_path, &hard_link_path).map_err(|e| {
            let err_msg = format!(
                "hard-link appendvec file {} to {} failed.  Error: {}",
                storage_path.display(),
                hard_link_path.display(),
                e,
            );
            SnapshotError::Io(IoError::new(ErrorKind::Other, err_msg))
        })?;
    }
    Ok(())
}

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
) -> Result<BankSnapshotInfo> {
    let mut add_snapshot_time = Measure::start("add-snapshot-ms");
    let slot = bank.slot();
    // bank_snapshots_dir/slot
    let bank_snapshot_dir = get_bank_snapshot_dir(&bank_snapshots_dir, slot);
    if bank_snapshot_dir.is_dir() {
        // There is a time window from when a snapshot directory is created to when its content
        // is fully filled to become a full state good to construct a bank from.  At the init time,
        // the system may not be booted from the latest snapshot directory, but an older and complete
        // directory.  Then, when adding new snapshots, the newer incomplete snapshot directory could
        // be found.  If so, it should be removed.
        purge_bank_snapshot(&bank_snapshot_dir)?;
    } else {
        // Even the snapshot directory is not found, still ensure the account snapshot directory
        // is also clean.  hardlink failure will happen if an old file exists.
        let account_paths = &bank.accounts().accounts_db.paths;
        let slot_str = slot.to_string();
        for account_path in account_paths {
            let account_snapshot_path = account_path
                .parent()
                .ok_or(SnapshotError::InvalidAccountPath(account_path.clone()))?
                .join("snapshot")
                .join(&slot_str);
            if account_snapshot_path.is_dir() {
                // remove the account snapshot directory
                move_and_async_delete_path(&account_snapshot_path);
            }
        }
    }
    fs::create_dir_all(&bank_snapshot_dir)?;

    // the bank snapshot is stored as bank_snapshots_dir/slot/slot.BANK_SNAPSHOT_PRE_FILENAME_EXTENSION
    let bank_snapshot_path = bank_snapshot_dir
        .join(get_snapshot_file_name(slot))
        .with_extension(BANK_SNAPSHOT_PRE_FILENAME_EXTENSION);

    info!(
        "Creating bank snapshot for slot {}, path: {}",
        slot,
        bank_snapshot_path.display(),
    );

    // We are constructing the snapshot directory to contain the full snapshot state information to allow
    // constructing a bank from this directory.  It acts like an archive to include the full state.
    // The set of the account appendvec files is the necessary part of this snapshot state.  Hard-link them
    // from the operational accounts/ directory to here.
    hard_link_storages_to_snapshot(&bank_snapshot_dir, slot, snapshot_storages)?;

    let bank_snapshot_serializer = move |stream: &mut BufWriter<File>| -> Result<()> {
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
    let (bank_snapshot_consumed_size, bank_serialize) = measure!(serialize_snapshot_data_file(
        &bank_snapshot_path,
        bank_snapshot_serializer
    )?);
    add_snapshot_time.stop();

    let status_cache_path = bank_snapshot_dir.join(SNAPSHOT_STATUS_CACHE_FILENAME);
    let (status_cache_consumed_size, status_cache_serialize) =
        measure!(serialize_status_cache(&slot_deltas, &status_cache_path)?);

    let version_path = bank_snapshot_dir.join(SNAPSHOT_VERSION_FILENAME);
    write_snapshot_version_file(version_path, snapshot_version).unwrap();

    // Mark this directory complete so it can be used.  Check this flag first before selecting for deserialization.
    let state_complete_path = bank_snapshot_dir.join(SNAPSHOT_STATE_COMPLETE_FILENAME);
    fs::File::create(state_complete_path)?;

    // Monitor sizes because they're capped to MAX_SNAPSHOT_DATA_FILE_SIZE
    datapoint_info!(
        "snapshot-bank-file",
        ("slot", slot, i64),
        ("bank_size", bank_snapshot_consumed_size, i64),
        ("status_cache_size", status_cache_consumed_size, i64),
        ("bank_serialize_ms", bank_serialize.as_ms(), i64),
        ("add_snapshot_ms", add_snapshot_time.as_ms(), i64),
        (
            "status_cache_serialize_ms",
            status_cache_serialize.as_ms(),
            i64
        ),
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
}

/// serializing needs Vec<Vec<Arc<AccountStorageEntry>>>, but data structure at runtime is Vec<Arc<AccountStorageEntry>>
/// translates to what we need
pub(crate) fn get_storages_to_serialize(
    snapshot_storages: &[Arc<AccountStorageEntry>],
) -> Vec<Vec<Arc<AccountStorageEntry>>> {
    snapshot_storages
        .iter()
        .map(|storage| vec![Arc::clone(storage)])
        .collect::<Vec<_>>()
}

fn serialize_status_cache(slot_deltas: &[BankSlotDelta], status_cache_path: &Path) -> Result<u64> {
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

// From testing, 4 seems to be a sweet spot for ranges of 60M-360M accounts and 16-64 cores. This may need to be tuned later.
const PARALLEL_UNTAR_READERS_DEFAULT: usize = 4;

fn verify_and_unarchive_snapshots(
    bank_snapshots_dir: impl AsRef<Path>,
    full_snapshot_archive_info: &FullSnapshotArchiveInfo,
    incremental_snapshot_archive_info: Option<&IncrementalSnapshotArchiveInfo>,
    account_paths: &[PathBuf],
) -> Result<(UnarchivedSnapshot, Option<UnarchivedSnapshot>, AtomicU32)> {
    check_are_snapshots_compatible(
        full_snapshot_archive_info,
        incremental_snapshot_archive_info,
    )?;

    let parallel_divisions = (num_cpus::get() / 4).clamp(1, PARALLEL_UNTAR_READERS_DEFAULT);

    let next_append_vec_id = Arc::new(AtomicAppendVecId::new(0));
    let unarchived_full_snapshot = unarchive_snapshot(
        &bank_snapshots_dir,
        TMP_SNAPSHOT_ARCHIVE_PREFIX,
        full_snapshot_archive_info.path(),
        "snapshot untar",
        account_paths,
        full_snapshot_archive_info.archive_format(),
        parallel_divisions,
        next_append_vec_id.clone(),
    )?;

    let unarchived_incremental_snapshot =
        if let Some(incremental_snapshot_archive_info) = incremental_snapshot_archive_info {
            let unarchived_incremental_snapshot = unarchive_snapshot(
                &bank_snapshots_dir,
                TMP_SNAPSHOT_ARCHIVE_PREFIX,
                incremental_snapshot_archive_info.path(),
                "incremental snapshot untar",
                account_paths,
                incremental_snapshot_archive_info.archive_format(),
                parallel_divisions,
                next_append_vec_id.clone(),
            )?;
            Some(unarchived_incremental_snapshot)
        } else {
            None
        };

    Ok((
        unarchived_full_snapshot,
        unarchived_incremental_snapshot,
        Arc::try_unwrap(next_append_vec_id).unwrap(),
    ))
}

/// Utility for parsing out bank specific information from a snapshot archive. This utility can be used
/// to parse out bank specific information like the leader schedule, epoch schedule, etc.
pub fn bank_fields_from_snapshot_archives(
    full_snapshot_archives_dir: impl AsRef<Path>,
    incremental_snapshot_archives_dir: impl AsRef<Path>,
) -> Result<BankFieldsToDeserialize> {
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
    additional_builtins: Option<&Builtins>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    test_hash_calculation: bool,
    accounts_db_skip_shrink: bool,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: &Arc<AtomicBool>,
) -> Result<(Bank, BankFromArchiveTimings)> {
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
        storage.extend(incremental_snapshot_storages.into_iter());
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
    Ok((bank, timings))
}

/// Rebuild bank from snapshot archives.  This function searches `full_snapshot_archives_dir` and `incremental_snapshot_archives_dir` for the
/// highest full snapshot and highest corresponding incremental snapshot, then rebuilds the bank.
#[allow(clippy::too_many_arguments)]
pub fn bank_from_latest_snapshot_archives(
    bank_snapshots_dir: impl AsRef<Path>,
    full_snapshot_archives_dir: impl AsRef<Path>,
    incremental_snapshot_archives_dir: impl AsRef<Path>,
    account_paths: &[PathBuf],
    genesis_config: &GenesisConfig,
    runtime_config: &RuntimeConfig,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&Builtins>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    test_hash_calculation: bool,
    accounts_db_skip_shrink: bool,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: &Arc<AtomicBool>,
) -> Result<(
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

    info!(
        "Loading bank from full snapshot: {}, and incremental snapshot: {:?}",
        full_snapshot_archive_info.path().display(),
        incremental_snapshot_archive_info
            .as_ref()
            .map(
                |incremental_snapshot_archive_info| incremental_snapshot_archive_info
                    .path()
                    .display()
            )
    );

    let (bank, timings) = bank_from_snapshot_archives(
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
        verify_index,
        accounts_db_config,
        accounts_update_notifier,
        exit,
    )?;

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
    additional_builtins: Option<&Builtins>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: &Arc<AtomicBool>,
) -> Result<(Bank, BankFromDirTimings)> {
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
    let mut measure_rebuild = Measure::start("rebuild bank from snapshots");
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
    additional_builtins: Option<&Builtins>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: &Arc<AtomicBool>,
) -> Result<Bank> {
    info!("Loading bank from snapshot dir");
    let bank_snapshot = get_highest_bank_snapshot_post(&bank_snapshots_dir).ok_or_else(|| {
        SnapshotError::NoSnapshotSlotDir(bank_snapshots_dir.as_ref().to_path_buf())
    })?;

    let (bank, timings) = bank_from_snapshot_dir(
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
    Ok(bank)
}

/// Check to make sure the deserialized bank's slot and hash matches the snapshot archive's slot
/// and hash
fn verify_bank_against_expected_slot_hash(
    bank: &Bank,
    expected_slot: Slot,
    expected_hash: SnapshotHash,
) -> Result<()> {
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

/// Spawns a thread for unpacking a snapshot
fn spawn_unpack_snapshot_thread(
    file_sender: Sender<PathBuf>,
    account_paths: Arc<Vec<PathBuf>>,
    ledger_dir: Arc<PathBuf>,
    mut archive: Archive<SharedBufferReader>,
    parallel_selector: Option<ParallelSelector>,
    thread_index: usize,
) -> JoinHandle<()> {
    Builder::new()
        .name(format!("solUnpkSnpsht{thread_index:02}"))
        .spawn(move || {
            streaming_unpack_snapshot(
                &mut archive,
                ledger_dir.as_path(),
                &account_paths,
                parallel_selector,
                &file_sender,
            )
            .unwrap();
        })
        .unwrap()
}

/// Streams unpacked files across channel
fn streaming_unarchive_snapshot(
    file_sender: Sender<PathBuf>,
    account_paths: Vec<PathBuf>,
    ledger_dir: PathBuf,
    snapshot_archive_path: PathBuf,
    archive_format: ArchiveFormat,
    num_threads: usize,
) -> Vec<JoinHandle<()>> {
    let account_paths = Arc::new(account_paths);
    let ledger_dir = Arc::new(ledger_dir);
    let shared_buffer = untar_snapshot_create_shared_buffer(&snapshot_archive_path, archive_format);

    // All shared buffer readers need to be created before the threads are spawned
    #[allow(clippy::needless_collect)]
    let archives: Vec<_> = (0..num_threads)
        .map(|_| {
            let reader = SharedBufferReader::new(&shared_buffer);
            Archive::new(reader)
        })
        .collect();

    archives
        .into_iter()
        .enumerate()
        .map(|(thread_index, archive)| {
            let parallel_selector = Some(ParallelSelector {
                index: thread_index,
                divisions: num_threads,
            });

            spawn_unpack_snapshot_thread(
                file_sender.clone(),
                account_paths.clone(),
                ledger_dir.clone(),
                archive,
                parallel_selector,
                thread_index,
            )
        })
        .collect()
}

/// BankSnapshotInfo::new_from_dir() requires a few meta files to accept a snapshot dir
/// as a valid one.  A dir unpacked from an archive lacks these files.  Fill them here to
/// allow new_from_dir() checks to pass.  These checks are not needed for unpacked dirs,
/// but it is not clean to add another flag to new_from_dir() to skip them.
fn create_snapshot_meta_files_for_unarchived_snapshot(unpack_dir: impl AsRef<Path>) -> Result<()> {
    let snapshots_dir = unpack_dir.as_ref().join("snapshots");
    if !snapshots_dir.is_dir() {
        return Err(SnapshotError::NoSnapshotSlotDir(snapshots_dir));
    }

    // The unpacked dir has a single slot dir, which is the snapshot slot dir.
    let slot_dir = fs::read_dir(&snapshots_dir)
        .map_err(|_| SnapshotError::NoSnapshotSlotDir(snapshots_dir.clone()))?
        .find(|entry| entry.as_ref().unwrap().path().is_dir())
        .ok_or_else(|| SnapshotError::NoSnapshotSlotDir(snapshots_dir.clone()))?
        .map_err(|_| SnapshotError::NoSnapshotSlotDir(snapshots_dir.clone()))?
        .path();

    let version_file = unpack_dir.as_ref().join(SNAPSHOT_VERSION_FILENAME);
    fs::hard_link(version_file, slot_dir.join(SNAPSHOT_VERSION_FILENAME))?;

    let status_cache_file = snapshots_dir.join(SNAPSHOT_STATUS_CACHE_FILENAME);
    fs::hard_link(
        status_cache_file,
        slot_dir.join(SNAPSHOT_STATUS_CACHE_FILENAME),
    )?;

    let state_complete_file = slot_dir.join(SNAPSHOT_STATE_COMPLETE_FILENAME);
    fs::File::create(state_complete_file)?;

    Ok(())
}

/// Perform the common tasks when unarchiving a snapshot.  Handles creating the temporary
/// directories, untaring, reading the version file, and then returning those fields plus the
/// rebuilt storage
fn unarchive_snapshot(
    bank_snapshots_dir: impl AsRef<Path>,
    unpacked_snapshots_dir_prefix: &'static str,
    snapshot_archive_path: impl AsRef<Path>,
    measure_name: &'static str,
    account_paths: &[PathBuf],
    archive_format: ArchiveFormat,
    parallel_divisions: usize,
    next_append_vec_id: Arc<AtomicAppendVecId>,
) -> Result<UnarchivedSnapshot> {
    let unpack_dir = tempfile::Builder::new()
        .prefix(unpacked_snapshots_dir_prefix)
        .tempdir_in(bank_snapshots_dir)?;
    let unpacked_snapshots_dir = unpack_dir.path().join("snapshots");

    let (file_sender, file_receiver) = crossbeam_channel::unbounded();
    streaming_unarchive_snapshot(
        file_sender,
        account_paths.to_vec(),
        unpack_dir.path().to_path_buf(),
        snapshot_archive_path.as_ref().to_path_buf(),
        archive_format,
        parallel_divisions,
    );

    let num_rebuilder_threads = num_cpus::get_physical()
        .saturating_sub(parallel_divisions)
        .max(1);
    let (version_and_storages, measure_untar) = measure!(
        SnapshotStorageRebuilder::rebuild_storage(
            file_receiver,
            num_rebuilder_threads,
            next_append_vec_id,
            SnapshotFrom::Archive,
        )?,
        measure_name
    );
    info!("{}", measure_untar);

    create_snapshot_meta_files_for_unarchived_snapshot(&unpack_dir)?;

    let RebuiltSnapshotStorage {
        snapshot_version,
        storage,
    } = version_and_storages;
    Ok(UnarchivedSnapshot {
        unpack_dir,
        storage,
        unpacked_snapshots_dir_and_version: UnpackedSnapshotsDirAndVersion {
            unpacked_snapshots_dir,
            snapshot_version,
        },
        measure_untar,
    })
}

/// Streams snapshot dir files across channel
/// Follow the flow of streaming_unarchive_snapshot(), but handle the from_dir case.
fn streaming_snapshot_dir_files(
    file_sender: Sender<PathBuf>,
    snapshot_file_path: impl Into<PathBuf>,
    snapshot_version_path: impl Into<PathBuf>,
    account_paths: &[PathBuf],
) -> Result<()> {
    file_sender.send(snapshot_file_path.into())?;
    file_sender.send(snapshot_version_path.into())?;

    for account_path in account_paths {
        for file in fs::read_dir(account_path)? {
            file_sender.send(file?.path())?;
        }
    }

    Ok(())
}

/// Perform the common tasks when deserialize a snapshot.  Handles reading snapshot file, reading the version file,
/// and then returning those fields plus the rebuilt storage
fn build_storage_from_snapshot_dir(
    snapshot_info: &BankSnapshotInfo,
    account_paths: &[PathBuf],
    next_append_vec_id: Arc<AtomicAppendVecId>,
) -> Result<AccountStorageMap> {
    let bank_snapshot_dir = &snapshot_info.snapshot_dir;
    let snapshot_file_path = &snapshot_info.snapshot_path();
    let snapshot_version_path = bank_snapshot_dir.join("version");
    let (file_sender, file_receiver) = crossbeam_channel::unbounded();

    let accounts_hardlinks = bank_snapshot_dir.join(SNAPSHOT_ACCOUNTS_HARDLINKS);

    let account_paths_set: HashSet<_> = HashSet::from_iter(account_paths.iter());

    for dir_entry in fs::read_dir(&accounts_hardlinks).map_err(|err| {
        SnapshotError::IoWithSourceAndFile(
            err,
            "read_dir failed for accounts_hardlinks",
            accounts_hardlinks.to_path_buf(),
        )
    })? {
        let symlink_path = dir_entry?.path();
        // The symlink point to <account_path>/snapshot/<slot> which contain the account files hardlinks
        // The corresponding run path should be <account_path>/run/
        let snapshot_account_path = fs::read_link(&symlink_path).map_err(|err| {
            SnapshotError::IoWithSourceAndFile(
                err,
                "read_link failed for symlink",
                symlink_path.to_path_buf(),
            )
        })?;
        let account_run_path = snapshot_account_path
            .parent()
            .ok_or_else(|| SnapshotError::InvalidAccountPath(snapshot_account_path.clone()))?
            .parent()
            .ok_or_else(|| SnapshotError::InvalidAccountPath(snapshot_account_path.clone()))?
            .join("run");
        if !account_paths_set.contains(&account_run_path) {
            // The appendvec from the bank snapshot stoarge does not match any of the provided account_paths set.
            // The accout paths have changed so the snapshot is no longer usable.
            return Err(SnapshotError::AccountPathsMismatch);
        }
        // Generate hard-links to make the account files available in the main accounts/, and let the new appendvec
        // paths be in accounts/
        for file in fs::read_dir(&snapshot_account_path).map_err(|err| {
            SnapshotError::IoWithSourceAndFile(
                err,
                "read_dir failed for snapshot_account_path",
                snapshot_account_path.to_path_buf(),
            )
        })? {
            let file_path = file?.path();
            let file_name = file_path
                .file_name()
                .ok_or_else(|| SnapshotError::InvalidAppendVecPath(file_path.to_path_buf()))?;
            let dest_path = account_run_path.clone().join(file_name);
            fs::hard_link(&file_path, &dest_path).map_err(|e| {
                let err_msg = format!(
                    "Error: {}.  Failed to hard-link {} to {}",
                    e,
                    file_path.display(),
                    dest_path.display()
                );
                SnapshotError::Io(IoError::new(ErrorKind::Other, err_msg))
            })?;
        }
    }

    streaming_snapshot_dir_files(
        file_sender,
        snapshot_file_path,
        snapshot_version_path,
        account_paths,
    )?;

    let num_rebuilder_threads = num_cpus::get_physical().saturating_sub(1).max(1);
    let version_and_storages = SnapshotStorageRebuilder::rebuild_storage(
        file_receiver,
        num_rebuilder_threads,
        next_append_vec_id,
        SnapshotFrom::Dir,
    )?;

    let RebuiltSnapshotStorage {
        snapshot_version: _,
        storage,
    } = version_and_storages;
    Ok(storage)
}

/// Reads the `snapshot_version` from a file. Before opening the file, its size
/// is compared to `MAX_SNAPSHOT_VERSION_FILE_SIZE`. If the size exceeds this
/// threshold, it is not opened and an error is returned.
fn snapshot_version_from_file(path: impl AsRef<Path>) -> Result<String> {
    // Check file size.
    let file_size = fs::metadata(&path)?.len();
    if file_size > MAX_SNAPSHOT_VERSION_FILE_SIZE {
        let error_message = format!(
            "snapshot version file too large: {} has {} bytes (max size is {} bytes)",
            path.as_ref().display(),
            file_size,
            MAX_SNAPSHOT_VERSION_FILE_SIZE,
        );
        return Err(get_io_error(&error_message));
    }

    // Read snapshot_version from file.
    let mut snapshot_version = String::new();
    File::open(path).and_then(|mut f| f.read_to_string(&mut snapshot_version))?;
    Ok(snapshot_version.trim().to_string())
}

/// Check if an incremental snapshot is compatible with a full snapshot.  This is done by checking
/// if the incremental snapshot's base slot is the same as the full snapshot's slot.
fn check_are_snapshots_compatible(
    full_snapshot_archive_info: &FullSnapshotArchiveInfo,
    incremental_snapshot_archive_info: Option<&IncrementalSnapshotArchiveInfo>,
) -> Result<()> {
    if incremental_snapshot_archive_info.is_none() {
        return Ok(());
    }

    let incremental_snapshot_archive_info = incremental_snapshot_archive_info.unwrap();

    (full_snapshot_archive_info.slot() == incremental_snapshot_archive_info.base_slot())
        .then_some(())
        .ok_or_else(|| {
            SnapshotError::MismatchedBaseSlot(
                full_snapshot_archive_info.slot(),
                incremental_snapshot_archive_info.base_slot(),
            )
        })
}

/// Get the `&str` from a `&Path`
pub fn path_to_file_name_str(path: &Path) -> Result<&str> {
    path.file_name()
        .ok_or_else(|| SnapshotError::PathToFileNameError(path.to_path_buf()))?
        .to_str()
        .ok_or_else(|| SnapshotError::FileNameToStrError(path.to_path_buf()))
}

pub fn build_snapshot_archives_remote_dir(snapshot_archives_dir: impl AsRef<Path>) -> PathBuf {
    snapshot_archives_dir
        .as_ref()
        .join(SNAPSHOT_ARCHIVE_DOWNLOAD_DIR)
}

/// Build the full snapshot archive path from its components: the snapshot archives directory, the
/// snapshot slot, the accounts hash, and the archive format.
pub fn build_full_snapshot_archive_path(
    full_snapshot_archives_dir: impl AsRef<Path>,
    slot: Slot,
    hash: &SnapshotHash,
    archive_format: ArchiveFormat,
) -> PathBuf {
    full_snapshot_archives_dir.as_ref().join(format!(
        "snapshot-{}-{}.{}",
        slot,
        hash.0,
        archive_format.extension(),
    ))
}

/// Build the incremental snapshot archive path from its components: the snapshot archives
/// directory, the snapshot base slot, the snapshot slot, the accounts hash, and the archive
/// format.
pub fn build_incremental_snapshot_archive_path(
    incremental_snapshot_archives_dir: impl AsRef<Path>,
    base_slot: Slot,
    slot: Slot,
    hash: &SnapshotHash,
    archive_format: ArchiveFormat,
) -> PathBuf {
    incremental_snapshot_archives_dir.as_ref().join(format!(
        "incremental-snapshot-{}-{}-{}.{}",
        base_slot,
        slot,
        hash.0,
        archive_format.extension(),
    ))
}

/// Parse a full snapshot archive filename into its Slot, Hash, and Archive Format
pub(crate) fn parse_full_snapshot_archive_filename(
    archive_filename: &str,
) -> Result<(Slot, SnapshotHash, ArchiveFormat)> {
    lazy_static! {
        static ref RE: Regex = Regex::new(FULL_SNAPSHOT_ARCHIVE_FILENAME_REGEX).unwrap();
    }

    let do_parse = || {
        RE.captures(archive_filename).and_then(|captures| {
            let slot = captures
                .name("slot")
                .map(|x| x.as_str().parse::<Slot>())?
                .ok()?;
            let hash = captures
                .name("hash")
                .map(|x| x.as_str().parse::<Hash>())?
                .ok()?;
            let archive_format = captures
                .name("ext")
                .map(|x| x.as_str().parse::<ArchiveFormat>())?
                .ok()?;

            Some((slot, SnapshotHash(hash), archive_format))
        })
    };

    do_parse().ok_or_else(|| {
        SnapshotError::ParseSnapshotArchiveFileNameError(archive_filename.to_string())
    })
}

/// Parse an incremental snapshot archive filename into its base Slot, actual Slot, Hash, and Archive Format
pub(crate) fn parse_incremental_snapshot_archive_filename(
    archive_filename: &str,
) -> Result<(Slot, Slot, SnapshotHash, ArchiveFormat)> {
    lazy_static! {
        static ref RE: Regex = Regex::new(INCREMENTAL_SNAPSHOT_ARCHIVE_FILENAME_REGEX).unwrap();
    }

    let do_parse = || {
        RE.captures(archive_filename).and_then(|captures| {
            let base_slot = captures
                .name("base")
                .map(|x| x.as_str().parse::<Slot>())?
                .ok()?;
            let slot = captures
                .name("slot")
                .map(|x| x.as_str().parse::<Slot>())?
                .ok()?;
            let hash = captures
                .name("hash")
                .map(|x| x.as_str().parse::<Hash>())?
                .ok()?;
            let archive_format = captures
                .name("ext")
                .map(|x| x.as_str().parse::<ArchiveFormat>())?
                .ok()?;

            Some((base_slot, slot, SnapshotHash(hash), archive_format))
        })
    };

    do_parse().ok_or_else(|| {
        SnapshotError::ParseSnapshotArchiveFileNameError(archive_filename.to_string())
    })
}

/// Walk down the snapshot archive to collect snapshot archive file info
fn get_snapshot_archives<T, F>(snapshot_archives_dir: &Path, cb: F) -> Vec<T>
where
    F: Fn(PathBuf) -> Result<T>,
{
    let walk_dir = |dir: &Path| -> Vec<T> {
        let entry_iter = fs::read_dir(dir);
        match entry_iter {
            Err(err) => {
                info!(
                    "Unable to read snapshot archives directory: err: {}, path: {}",
                    err,
                    dir.display()
                );
                vec![]
            }
            Ok(entries) => entries
                .filter_map(|entry| entry.map_or(None, |entry| cb(entry.path()).ok()))
                .collect(),
        }
    };

    let mut ret = walk_dir(snapshot_archives_dir);
    let remote_dir = build_snapshot_archives_remote_dir(snapshot_archives_dir);
    if remote_dir.exists() {
        ret.append(&mut walk_dir(remote_dir.as_ref()));
    }
    ret
}

/// Get a list of the full snapshot archives from a directory
pub fn get_full_snapshot_archives(
    full_snapshot_archives_dir: impl AsRef<Path>,
) -> Vec<FullSnapshotArchiveInfo> {
    get_snapshot_archives(
        full_snapshot_archives_dir.as_ref(),
        FullSnapshotArchiveInfo::new_from_path,
    )
}

/// Get a list of the incremental snapshot archives from a directory
pub fn get_incremental_snapshot_archives(
    incremental_snapshot_archives_dir: impl AsRef<Path>,
) -> Vec<IncrementalSnapshotArchiveInfo> {
    get_snapshot_archives(
        incremental_snapshot_archives_dir.as_ref(),
        IncrementalSnapshotArchiveInfo::new_from_path,
    )
}

/// Get the highest slot of the full snapshot archives in a directory
pub fn get_highest_full_snapshot_archive_slot(
    full_snapshot_archives_dir: impl AsRef<Path>,
) -> Option<Slot> {
    get_highest_full_snapshot_archive_info(full_snapshot_archives_dir)
        .map(|full_snapshot_archive_info| full_snapshot_archive_info.slot())
}

/// Get the highest slot of the incremental snapshot archives in a directory, for a given full
/// snapshot slot
pub fn get_highest_incremental_snapshot_archive_slot(
    incremental_snapshot_archives_dir: impl AsRef<Path>,
    full_snapshot_slot: Slot,
) -> Option<Slot> {
    get_highest_incremental_snapshot_archive_info(
        incremental_snapshot_archives_dir,
        full_snapshot_slot,
    )
    .map(|incremental_snapshot_archive_info| incremental_snapshot_archive_info.slot())
}

/// Get the path (and metadata) for the full snapshot archive with the highest slot in a directory
pub fn get_highest_full_snapshot_archive_info(
    full_snapshot_archives_dir: impl AsRef<Path>,
) -> Option<FullSnapshotArchiveInfo> {
    let mut full_snapshot_archives = get_full_snapshot_archives(full_snapshot_archives_dir);
    full_snapshot_archives.sort_unstable();
    full_snapshot_archives.into_iter().rev().next()
}

/// Get the path for the incremental snapshot archive with the highest slot, for a given full
/// snapshot slot, in a directory
pub fn get_highest_incremental_snapshot_archive_info(
    incremental_snapshot_archives_dir: impl AsRef<Path>,
    full_snapshot_slot: Slot,
) -> Option<IncrementalSnapshotArchiveInfo> {
    // Since we want to filter down to only the incremental snapshot archives that have the same
    // full snapshot slot as the value passed in, perform the filtering before sorting to avoid
    // doing unnecessary work.
    let mut incremental_snapshot_archives =
        get_incremental_snapshot_archives(incremental_snapshot_archives_dir)
            .into_iter()
            .filter(|incremental_snapshot_archive_info| {
                incremental_snapshot_archive_info.base_slot() == full_snapshot_slot
            })
            .collect::<Vec<_>>();
    incremental_snapshot_archives.sort_unstable();
    incremental_snapshot_archives.into_iter().rev().next()
}

pub fn purge_old_snapshot_archives(
    full_snapshot_archives_dir: impl AsRef<Path>,
    incremental_snapshot_archives_dir: impl AsRef<Path>,
    maximum_full_snapshot_archives_to_retain: NonZeroUsize,
    maximum_incremental_snapshot_archives_to_retain: NonZeroUsize,
) {
    info!(
        "Purging old full snapshot archives in {}, retaining up to {} full snapshots",
        full_snapshot_archives_dir.as_ref().display(),
        maximum_full_snapshot_archives_to_retain
    );

    let mut full_snapshot_archives = get_full_snapshot_archives(&full_snapshot_archives_dir);
    full_snapshot_archives.sort_unstable();
    full_snapshot_archives.reverse();

    let num_to_retain = full_snapshot_archives
        .len()
        .min(maximum_full_snapshot_archives_to_retain.get());
    trace!(
        "There are {} full snapshot archives, retaining {}",
        full_snapshot_archives.len(),
        num_to_retain,
    );

    let (full_snapshot_archives_to_retain, full_snapshot_archives_to_remove) =
        if full_snapshot_archives.is_empty() {
            None
        } else {
            Some(full_snapshot_archives.split_at(num_to_retain))
        }
        .unwrap_or_default();

    let retained_full_snapshot_slots = full_snapshot_archives_to_retain
        .iter()
        .map(|ai| ai.slot())
        .collect::<HashSet<_>>();

    fn remove_archives<T: SnapshotArchiveInfoGetter>(archives: &[T]) {
        for path in archives.iter().map(|a| a.path()) {
            trace!("Removing snapshot archive: {}", path.display());
            fs::remove_file(path)
                .unwrap_or_else(|err| info!("Failed to remove {}: {}", path.display(), err));
        }
    }
    remove_archives(full_snapshot_archives_to_remove);

    info!(
        "Purging old incremental snapshot archives in {}, retaining up to {} incremental snapshots",
        incremental_snapshot_archives_dir.as_ref().display(),
        maximum_incremental_snapshot_archives_to_retain
    );
    let mut incremental_snapshot_archives_by_base_slot = HashMap::<Slot, Vec<_>>::new();
    for incremental_snapshot_archive in
        get_incremental_snapshot_archives(&incremental_snapshot_archives_dir)
    {
        incremental_snapshot_archives_by_base_slot
            .entry(incremental_snapshot_archive.base_slot())
            .or_default()
            .push(incremental_snapshot_archive)
    }

    let highest_full_snapshot_slot = retained_full_snapshot_slots.iter().max().copied();
    for (base_slot, mut incremental_snapshot_archives) in incremental_snapshot_archives_by_base_slot
    {
        incremental_snapshot_archives.sort_unstable();
        let num_to_retain = if Some(base_slot) == highest_full_snapshot_slot {
            maximum_incremental_snapshot_archives_to_retain.get()
        } else {
            usize::from(retained_full_snapshot_slots.contains(&base_slot))
        };
        trace!(
            "There are {} incremental snapshot archives for base slot {}, removing {} of them",
            incremental_snapshot_archives.len(),
            base_slot,
            incremental_snapshot_archives
                .len()
                .saturating_sub(num_to_retain),
        );

        incremental_snapshot_archives.truncate(
            incremental_snapshot_archives
                .len()
                .saturating_sub(num_to_retain),
        );
        remove_archives(&incremental_snapshot_archives);
    }
}

fn unpack_snapshot_local(
    shared_buffer: SharedBuffer,
    ledger_dir: &Path,
    account_paths: &[PathBuf],
    parallel_divisions: usize,
) -> Result<UnpackedAppendVecMap> {
    assert!(parallel_divisions > 0);

    // allocate all readers before any readers start reading
    let readers = (0..parallel_divisions)
        .map(|_| SharedBufferReader::new(&shared_buffer))
        .collect::<Vec<_>>();

    // create 'parallel_divisions' # of parallel workers, each responsible for 1/parallel_divisions of all the files to extract.
    let all_unpacked_append_vec_map = readers
        .into_par_iter()
        .enumerate()
        .map(|(index, reader)| {
            let parallel_selector = Some(ParallelSelector {
                index,
                divisions: parallel_divisions,
            });
            let mut archive = Archive::new(reader);
            unpack_snapshot(&mut archive, ledger_dir, account_paths, parallel_selector)
        })
        .collect::<Vec<_>>();

    let mut unpacked_append_vec_map = UnpackedAppendVecMap::new();
    for h in all_unpacked_append_vec_map {
        unpacked_append_vec_map.extend(h?);
    }

    Ok(unpacked_append_vec_map)
}

fn untar_snapshot_create_shared_buffer(
    snapshot_tar: &Path,
    archive_format: ArchiveFormat,
) -> SharedBuffer {
    let open_file = || File::open(snapshot_tar).unwrap();
    match archive_format {
        ArchiveFormat::TarBzip2 => SharedBuffer::new(BzDecoder::new(BufReader::new(open_file()))),
        ArchiveFormat::TarGzip => SharedBuffer::new(GzDecoder::new(BufReader::new(open_file()))),
        ArchiveFormat::TarZstd => SharedBuffer::new(
            zstd::stream::read::Decoder::new(BufReader::new(open_file())).unwrap(),
        ),
        ArchiveFormat::TarLz4 => {
            SharedBuffer::new(lz4::Decoder::new(BufReader::new(open_file())).unwrap())
        }
        ArchiveFormat::Tar => SharedBuffer::new(BufReader::new(open_file())),
    }
}

fn untar_snapshot_in(
    snapshot_tar: impl AsRef<Path>,
    unpack_dir: &Path,
    account_paths: &[PathBuf],
    archive_format: ArchiveFormat,
    parallel_divisions: usize,
) -> Result<UnpackedAppendVecMap> {
    let shared_buffer = untar_snapshot_create_shared_buffer(snapshot_tar.as_ref(), archive_format);
    unpack_snapshot_local(shared_buffer, unpack_dir, account_paths, parallel_divisions)
}

fn verify_unpacked_snapshots_dir_and_version(
    unpacked_snapshots_dir_and_version: &UnpackedSnapshotsDirAndVersion,
) -> Result<(SnapshotVersion, BankSnapshotInfo)> {
    info!(
        "snapshot version: {}",
        &unpacked_snapshots_dir_and_version.snapshot_version
    );

    let snapshot_version = unpacked_snapshots_dir_and_version.snapshot_version;
    let mut bank_snapshots =
        get_bank_snapshots_post(&unpacked_snapshots_dir_and_version.unpacked_snapshots_dir);
    if bank_snapshots.len() > 1 {
        return Err(get_io_error("invalid snapshot format"));
    }
    let root_paths = bank_snapshots
        .pop()
        .ok_or_else(|| get_io_error("No snapshots found in snapshots directory"))?;
    Ok((snapshot_version, root_paths))
}

fn bank_fields_from_snapshots(
    full_snapshot_unpacked_snapshots_dir_and_version: &UnpackedSnapshotsDirAndVersion,
    incremental_snapshot_unpacked_snapshots_dir_and_version: Option<
        &UnpackedSnapshotsDirAndVersion,
    >,
) -> Result<BankFieldsToDeserialize> {
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

fn deserialize_status_cache(status_cache_path: &Path) -> Result<Vec<BankSlotDelta>> {
    deserialize_snapshot_data_file(status_cache_path, |stream| {
        info!(
            "Rebuilding status cache from {}",
            status_cache_path.display()
        );
        let slot_delta: Vec<BankSlotDelta> = bincode::options()
            .with_limit(MAX_SNAPSHOT_DATA_FILE_SIZE)
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
    additional_builtins: Option<&Builtins>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: &Arc<AtomicBool>,
) -> Result<Bank> {
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
        .join(SNAPSHOT_STATUS_CACHE_FILENAME);
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
    additional_builtins: Option<&Builtins>,
    account_secondary_indexes: AccountSecondaryIndexes,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
    verify_index: bool,
    accounts_db_config: Option<AccountsDbConfig>,
    accounts_update_notifier: Option<AccountsUpdateNotifier>,
    exit: &Arc<AtomicBool>,
) -> Result<Bank> {
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
        .join(SNAPSHOT_STATUS_CACHE_FILENAME);
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

/// Returns the file name of the bank snapshot for `slot`
pub fn get_snapshot_file_name(slot: Slot) -> String {
    slot.to_string()
}

/// Constructs the path to the bank snapshot directory for `slot` within `bank_snapshots_dir`
pub fn get_bank_snapshot_dir(bank_snapshots_dir: impl AsRef<Path>, slot: Slot) -> PathBuf {
    bank_snapshots_dir
        .as_ref()
        .join(get_snapshot_file_name(slot))
}

fn get_io_error(error: &str) -> SnapshotError {
    warn!("Snapshot Error: {:?}", error);
    SnapshotError::Io(IoError::new(ErrorKind::Other, error))
}

#[derive(Debug, Copy, Clone)]
/// allow tests to specify what happened to the serialized format
pub enum VerifyBank {
    /// the bank's serialized format is expected to be identical to what we are comparing against
    Deterministic,
    /// the serialized bank was 'reserialized' into a non-deterministic format
    /// so, deserialize both files and compare deserialized results
    NonDeterministic,
}

pub fn verify_snapshot_archive(
    snapshot_archive: impl AsRef<Path>,
    snapshots_to_verify: impl AsRef<Path>,
    archive_format: ArchiveFormat,
    verify_bank: VerifyBank,
    slot: Slot,
) {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let unpack_dir = temp_dir.path();
    let unpack_account_dir = create_accounts_run_and_snapshot_dirs(unpack_dir).unwrap().0;
    untar_snapshot_in(
        snapshot_archive,
        unpack_dir,
        &[unpack_account_dir.clone()],
        archive_format,
        1,
    )
    .unwrap();

    // Check snapshots are the same
    let unpacked_snapshots = unpack_dir.join("snapshots");

    // Since the unpack code collects all the appendvecs into one directory unpack_account_dir, we need to
    // collect all the appendvecs in account_paths/<slot>/snapshot/ into one directory for later comparison.
    let storages_to_verify = unpack_dir.join("storages_to_verify");
    // Create the directory if it doesn't exist
    std::fs::create_dir_all(&storages_to_verify).unwrap();

    let slot = slot.to_string();
    let snapshot_slot_dir = snapshots_to_verify.as_ref().join(&slot);

    if let VerifyBank::NonDeterministic = verify_bank {
        // file contents may be different, but deserialized structs should be equal
        let p1 = snapshots_to_verify.as_ref().join(&slot).join(&slot);
        let p2 = unpacked_snapshots.join(&slot).join(&slot);
        assert!(crate::serde_snapshot::compare_two_serialized_banks(&p1, &p2).unwrap());
        std::fs::remove_file(p1).unwrap();
        std::fs::remove_file(p2).unwrap();
    }

    // The new the status_cache file is inside the slot directory together with the snapshot file.
    // When unpacking an archive, the status_cache file from the archive is one-level up outside of
    //  the slot direcotry.
    // The unpacked status_cache file need to be put back into the slot directory for the directory
    // comparison to pass.
    let existing_unpacked_status_cache_file =
        unpacked_snapshots.join(SNAPSHOT_STATUS_CACHE_FILENAME);
    let new_unpacked_status_cache_file = unpacked_snapshots
        .join(&slot)
        .join(SNAPSHOT_STATUS_CACHE_FILENAME);
    fs::rename(
        existing_unpacked_status_cache_file,
        new_unpacked_status_cache_file,
    )
    .unwrap();

    let accounts_hardlinks_dir = snapshot_slot_dir.join(SNAPSHOT_ACCOUNTS_HARDLINKS);
    if accounts_hardlinks_dir.is_dir() {
        // This directory contain symlinks to all <account_path>/snapshot/<slot> directories.
        for entry in fs::read_dir(&accounts_hardlinks_dir).unwrap() {
            let link_dst_path = fs::read_link(entry.unwrap().path()).unwrap();
            // Copy all the files in dst_path into the storages_to_verify directory.
            for entry in fs::read_dir(&link_dst_path).unwrap() {
                let src_path = entry.unwrap().path();
                let dst_path = storages_to_verify.join(src_path.file_name().unwrap());
                fs::copy(src_path, dst_path).unwrap();
            }
        }
        std::fs::remove_dir_all(accounts_hardlinks_dir).unwrap();
    }

    let version_path = snapshot_slot_dir.join(SNAPSHOT_VERSION_FILENAME);
    if version_path.is_file() {
        std::fs::remove_file(version_path).unwrap();
    }

    let state_complete_path = snapshot_slot_dir.join(SNAPSHOT_STATE_COMPLETE_FILENAME);
    if state_complete_path.is_file() {
        std::fs::remove_file(state_complete_path).unwrap();
    }

    assert!(!dir_diff::is_different(&snapshots_to_verify, unpacked_snapshots).unwrap());

    // In the unarchiving case, there is an extra empty "accounts" directory. The account
    // files in the archive accounts/ have been expanded to [account_paths].
    // Remove the empty "accounts" directory for the directory comparison below.
    // In some test cases the directory to compare do not come from unarchiving.
    // Ignore the error when this directory does not exist.
    _ = std::fs::remove_dir(unpack_account_dir.join("accounts"));
    // Check the account entries are the same
    assert!(!dir_diff::is_different(&storages_to_verify, unpack_account_dir).unwrap());
}

/// Remove outdated bank snapshots
pub fn purge_old_bank_snapshots(
    bank_snapshots_dir: impl AsRef<Path>,
    num_bank_snapshots_to_retain: usize,
    filter_by_type: Option<BankSnapshotType>,
) {
    let do_purge = |mut bank_snapshots: Vec<BankSnapshotInfo>| {
        bank_snapshots.sort_unstable();
        bank_snapshots
            .into_iter()
            .rev()
            .skip(num_bank_snapshots_to_retain)
            .for_each(|bank_snapshot| {
                let r = purge_bank_snapshot(&bank_snapshot.snapshot_dir);
                if r.is_err() {
                    warn!(
                        "Couldn't purge bank snapshot at: {}",
                        bank_snapshot.snapshot_dir.display()
                    );
                }
            })
    };

    let bank_snapshots = match filter_by_type {
        Some(BankSnapshotType::Pre) => get_bank_snapshots_pre(&bank_snapshots_dir),
        Some(BankSnapshotType::Post) => get_bank_snapshots_post(&bank_snapshots_dir),
        None => get_bank_snapshots(&bank_snapshots_dir),
    };
    do_purge(bank_snapshots);
}

/// Remove the bank snapshot at this path
fn purge_bank_snapshot(bank_snapshot_dir: impl AsRef<Path>) -> Result<()> {
    let accounts_hardlinks_dir = bank_snapshot_dir.as_ref().join(SNAPSHOT_ACCOUNTS_HARDLINKS);
    if accounts_hardlinks_dir.is_dir() {
        // This directory contain symlinks to all accounts snapshot directories.
        // They should all be removed.
        for accounts_hardlink_dir in fs::read_dir(accounts_hardlinks_dir)? {
            let accounts_hardlink_dir = fs::read_link(accounts_hardlink_dir?.path())?;
            move_and_async_delete_path(&accounts_hardlink_dir);
        }
    }
    fs::remove_dir_all(bank_snapshot_dir)?;
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
) -> Result<FullSnapshotArchiveInfo> {
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
) -> Result<IncrementalSnapshotArchiveInfo> {
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
) -> Result<FullSnapshotArchiveInfo> {
    let accounts_package = AccountsPackage::new_for_snapshot(
        AccountsPackageType::Snapshot(SnapshotType::FullSnapshot),
        bank,
        bank_snapshot_info,
        &full_snapshot_archives_dir,
        &incremental_snapshot_archives_dir,
        snapshot_storages,
        archive_format,
        snapshot_version,
        None,
    )?;

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
) -> Result<IncrementalSnapshotArchiveInfo> {
    let accounts_package = AccountsPackage::new_for_snapshot(
        AccountsPackageType::Snapshot(SnapshotType::IncrementalSnapshot(
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
    )?;

    let (accounts_hash_enum, accounts_hash_for_reserialize, bank_incremental_snapshot_persistence) =
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

    let snapshot_package = SnapshotPackage::new(accounts_package, accounts_hash_enum);
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

pub fn should_take_full_snapshot(
    block_height: Slot,
    full_snapshot_archive_interval_slots: Slot,
) -> bool {
    block_height % full_snapshot_archive_interval_slots == 0
}

pub fn should_take_incremental_snapshot(
    block_height: Slot,
    incremental_snapshot_archive_interval_slots: Slot,
    last_full_snapshot_slot: Option<Slot>,
) -> bool {
    block_height % incremental_snapshot_archive_interval_slots == 0
        && last_full_snapshot_slot.is_some()
}

pub fn create_tmp_accounts_dir_for_tests() -> (TempDir, PathBuf) {
    let tmp_dir = tempfile::TempDir::new().unwrap();
    let account_dir = create_accounts_run_and_snapshot_dirs(&tmp_dir).unwrap().0;
    (tmp_dir, account_dir)
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
        bank = Arc::new(Bank::new_from_parent(&bank, &collecter_id, bank.slot() + 1));
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
            accounts_db::ACCOUNTS_DB_CONFIG_FOR_TESTING,
            accounts_hash::{CalcAccountsHashConfig, HashStats},
            genesis_utils,
            snapshot_utils::snapshot_storage_rebuilder::get_slot_and_append_vec_id,
            sorted_storages::SortedStorages,
            status_cache::Status,
        },
        assert_matches::assert_matches,
        bincode::{deserialize_from, serialize_into},
        solana_sdk::{
            genesis_config::create_genesis_config,
            native_token::{sol_to_lamports, LAMPORTS_PER_SOL},
            signature::{Keypair, Signer},
            slot_history::SlotHistory,
            system_transaction,
            transaction::SanitizedTransaction,
        },
        std::{
            convert::TryFrom,
            mem::size_of,
            os::unix::fs::PermissionsExt,
            sync::{atomic::Ordering, Arc},
        },
        tempfile::NamedTempFile,
    };
    #[test]
    fn test_serialize_snapshot_data_file_under_limit() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let expected_consumed_size = size_of::<u32>() as u64;
        let consumed_size = serialize_snapshot_data_file_capped(
            &temp_dir.path().join("data-file"),
            expected_consumed_size,
            |stream| {
                serialize_into(stream, &2323_u32)?;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(consumed_size, expected_consumed_size);
    }

    #[test]
    fn test_serialize_snapshot_data_file_over_limit() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let expected_consumed_size = size_of::<u32>() as u64;
        let result = serialize_snapshot_data_file_capped(
            &temp_dir.path().join("data-file"),
            expected_consumed_size - 1,
            |stream| {
                serialize_into(stream, &2323_u32)?;
                Ok(())
            },
        );
        assert_matches!(result, Err(SnapshotError::Io(ref message)) if message.to_string().starts_with("too large snapshot data file to serialize"));
    }

    #[test]
    fn test_deserialize_snapshot_data_file_under_limit() {
        let expected_data = 2323_u32;
        let expected_consumed_size = size_of::<u32>() as u64;

        let temp_dir = tempfile::TempDir::new().unwrap();
        serialize_snapshot_data_file_capped(
            &temp_dir.path().join("data-file"),
            expected_consumed_size,
            |stream| {
                serialize_into(stream, &expected_data)?;
                Ok(())
            },
        )
        .unwrap();

        let snapshot_root_paths = SnapshotRootPaths {
            full_snapshot_root_file_path: temp_dir.path().join("data-file"),
            incremental_snapshot_root_file_path: None,
        };

        let actual_data = deserialize_snapshot_data_files_capped(
            &snapshot_root_paths,
            expected_consumed_size,
            |stream| {
                Ok(deserialize_from::<_, u32>(
                    &mut stream.full_snapshot_stream,
                )?)
            },
        )
        .unwrap();
        assert_eq!(actual_data, expected_data);
    }

    #[test]
    fn test_deserialize_snapshot_data_file_over_limit() {
        let expected_data = 2323_u32;
        let expected_consumed_size = size_of::<u32>() as u64;

        let temp_dir = tempfile::TempDir::new().unwrap();
        serialize_snapshot_data_file_capped(
            &temp_dir.path().join("data-file"),
            expected_consumed_size,
            |stream| {
                serialize_into(stream, &expected_data)?;
                Ok(())
            },
        )
        .unwrap();

        let snapshot_root_paths = SnapshotRootPaths {
            full_snapshot_root_file_path: temp_dir.path().join("data-file"),
            incremental_snapshot_root_file_path: None,
        };

        let result = deserialize_snapshot_data_files_capped(
            &snapshot_root_paths,
            expected_consumed_size - 1,
            |stream| {
                Ok(deserialize_from::<_, u32>(
                    &mut stream.full_snapshot_stream,
                )?)
            },
        );
        assert_matches!(result, Err(SnapshotError::Io(ref message)) if message.to_string().starts_with("too large snapshot data file to deserialize"));
    }

    #[test]
    fn test_deserialize_snapshot_data_file_extra_data() {
        let expected_data = 2323_u32;
        let expected_consumed_size = size_of::<u32>() as u64;

        let temp_dir = tempfile::TempDir::new().unwrap();
        serialize_snapshot_data_file_capped(
            &temp_dir.path().join("data-file"),
            expected_consumed_size * 2,
            |stream| {
                serialize_into(stream.by_ref(), &expected_data)?;
                serialize_into(stream.by_ref(), &expected_data)?;
                Ok(())
            },
        )
        .unwrap();

        let snapshot_root_paths = SnapshotRootPaths {
            full_snapshot_root_file_path: temp_dir.path().join("data-file"),
            incremental_snapshot_root_file_path: None,
        };

        let result = deserialize_snapshot_data_files_capped(
            &snapshot_root_paths,
            expected_consumed_size * 2,
            |stream| {
                Ok(deserialize_from::<_, u32>(
                    &mut stream.full_snapshot_stream,
                )?)
            },
        );
        assert_matches!(result, Err(SnapshotError::Io(ref message)) if message.to_string().starts_with("invalid snapshot data file"));
    }

    #[test]
    fn test_snapshot_version_from_file_under_limit() {
        let file_content = SnapshotVersion::default().as_str();
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(file_content.as_bytes()).unwrap();
        let version_from_file = snapshot_version_from_file(file.path()).unwrap();
        assert_eq!(version_from_file, file_content);
    }

    #[test]
    fn test_snapshot_version_from_file_over_limit() {
        let over_limit_size = usize::try_from(MAX_SNAPSHOT_VERSION_FILE_SIZE + 1).unwrap();
        let file_content = vec![7u8; over_limit_size];
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&file_content).unwrap();
        assert_matches!(
            snapshot_version_from_file(file.path()),
            Err(SnapshotError::Io(ref message)) if message.to_string().starts_with("snapshot version file too large")
        );
    }

    #[test]
    fn test_parse_full_snapshot_archive_filename() {
        assert_eq!(
            parse_full_snapshot_archive_filename(&format!(
                "snapshot-42-{}.tar.bz2",
                Hash::default()
            ))
            .unwrap(),
            (42, SnapshotHash(Hash::default()), ArchiveFormat::TarBzip2)
        );
        assert_eq!(
            parse_full_snapshot_archive_filename(&format!(
                "snapshot-43-{}.tar.zst",
                Hash::default()
            ))
            .unwrap(),
            (43, SnapshotHash(Hash::default()), ArchiveFormat::TarZstd)
        );
        assert_eq!(
            parse_full_snapshot_archive_filename(&format!("snapshot-44-{}.tar", Hash::default()))
                .unwrap(),
            (44, SnapshotHash(Hash::default()), ArchiveFormat::Tar)
        );
        assert_eq!(
            parse_full_snapshot_archive_filename(&format!(
                "snapshot-45-{}.tar.lz4",
                Hash::default()
            ))
            .unwrap(),
            (45, SnapshotHash(Hash::default()), ArchiveFormat::TarLz4)
        );

        assert!(parse_full_snapshot_archive_filename("invalid").is_err());
        assert!(
            parse_full_snapshot_archive_filename("snapshot-bad!slot-bad!hash.bad!ext").is_err()
        );

        assert!(
            parse_full_snapshot_archive_filename("snapshot-12345678-bad!hash.bad!ext").is_err()
        );
        assert!(parse_full_snapshot_archive_filename(&format!(
            "snapshot-12345678-{}.bad!ext",
            Hash::new_unique()
        ))
        .is_err());
        assert!(parse_full_snapshot_archive_filename("snapshot-12345678-bad!hash.tar").is_err());

        assert!(parse_full_snapshot_archive_filename(&format!(
            "snapshot-bad!slot-{}.bad!ext",
            Hash::new_unique()
        ))
        .is_err());
        assert!(parse_full_snapshot_archive_filename(&format!(
            "snapshot-12345678-{}.bad!ext",
            Hash::new_unique()
        ))
        .is_err());
        assert!(parse_full_snapshot_archive_filename(&format!(
            "snapshot-bad!slot-{}.tar",
            Hash::new_unique()
        ))
        .is_err());

        assert!(parse_full_snapshot_archive_filename("snapshot-bad!slot-bad!hash.tar").is_err());
        assert!(parse_full_snapshot_archive_filename("snapshot-12345678-bad!hash.tar").is_err());
        assert!(parse_full_snapshot_archive_filename(&format!(
            "snapshot-bad!slot-{}.tar",
            Hash::new_unique()
        ))
        .is_err());
    }

    #[test]
    fn test_parse_incremental_snapshot_archive_filename() {
        solana_logger::setup();
        assert_eq!(
            parse_incremental_snapshot_archive_filename(&format!(
                "incremental-snapshot-42-123-{}.tar.bz2",
                Hash::default()
            ))
            .unwrap(),
            (
                42,
                123,
                SnapshotHash(Hash::default()),
                ArchiveFormat::TarBzip2
            )
        );
        assert_eq!(
            parse_incremental_snapshot_archive_filename(&format!(
                "incremental-snapshot-43-234-{}.tar.zst",
                Hash::default()
            ))
            .unwrap(),
            (
                43,
                234,
                SnapshotHash(Hash::default()),
                ArchiveFormat::TarZstd
            )
        );
        assert_eq!(
            parse_incremental_snapshot_archive_filename(&format!(
                "incremental-snapshot-44-345-{}.tar",
                Hash::default()
            ))
            .unwrap(),
            (44, 345, SnapshotHash(Hash::default()), ArchiveFormat::Tar)
        );
        assert_eq!(
            parse_incremental_snapshot_archive_filename(&format!(
                "incremental-snapshot-45-456-{}.tar.lz4",
                Hash::default()
            ))
            .unwrap(),
            (
                45,
                456,
                SnapshotHash(Hash::default()),
                ArchiveFormat::TarLz4
            )
        );

        assert!(parse_incremental_snapshot_archive_filename("invalid").is_err());
        assert!(parse_incremental_snapshot_archive_filename(&format!(
            "snapshot-42-{}.tar",
            Hash::new_unique()
        ))
        .is_err());
        assert!(parse_incremental_snapshot_archive_filename(
            "incremental-snapshot-bad!slot-bad!slot-bad!hash.bad!ext"
        )
        .is_err());

        assert!(parse_incremental_snapshot_archive_filename(&format!(
            "incremental-snapshot-bad!slot-56785678-{}.tar",
            Hash::new_unique()
        ))
        .is_err());

        assert!(parse_incremental_snapshot_archive_filename(&format!(
            "incremental-snapshot-12345678-bad!slot-{}.tar",
            Hash::new_unique()
        ))
        .is_err());

        assert!(parse_incremental_snapshot_archive_filename(
            "incremental-snapshot-12341234-56785678-bad!HASH.tar"
        )
        .is_err());

        assert!(parse_incremental_snapshot_archive_filename(&format!(
            "incremental-snapshot-12341234-56785678-{}.bad!ext",
            Hash::new_unique()
        ))
        .is_err());
    }

    #[test]
    fn test_check_are_snapshots_compatible() {
        solana_logger::setup();
        let slot1: Slot = 1234;
        let slot2: Slot = 5678;
        let slot3: Slot = 999_999;

        let full_snapshot_archive_info = FullSnapshotArchiveInfo::new_from_path(PathBuf::from(
            format!("/dir/snapshot-{}-{}.tar", slot1, Hash::new_unique()),
        ))
        .unwrap();

        assert!(check_are_snapshots_compatible(&full_snapshot_archive_info, None,).is_ok());

        let incremental_snapshot_archive_info =
            IncrementalSnapshotArchiveInfo::new_from_path(PathBuf::from(format!(
                "/dir/incremental-snapshot-{}-{}-{}.tar",
                slot1,
                slot2,
                Hash::new_unique()
            )))
            .unwrap();

        assert!(check_are_snapshots_compatible(
            &full_snapshot_archive_info,
            Some(&incremental_snapshot_archive_info)
        )
        .is_ok());

        let incremental_snapshot_archive_info =
            IncrementalSnapshotArchiveInfo::new_from_path(PathBuf::from(format!(
                "/dir/incremental-snapshot-{}-{}-{}.tar",
                slot2,
                slot3,
                Hash::new_unique()
            )))
            .unwrap();

        assert!(check_are_snapshots_compatible(
            &full_snapshot_archive_info,
            Some(&incremental_snapshot_archive_info)
        )
        .is_err());
    }

    /// A test heler function that creates bank snapshot files
    fn common_create_bank_snapshot_files(
        bank_snapshots_dir: &Path,
        min_slot: Slot,
        max_slot: Slot,
    ) {
        for slot in min_slot..max_slot {
            let snapshot_dir = get_bank_snapshot_dir(bank_snapshots_dir, slot);
            fs::create_dir_all(&snapshot_dir).unwrap();

            let snapshot_filename = get_snapshot_file_name(slot);
            let snapshot_path = snapshot_dir.join(snapshot_filename);
            File::create(snapshot_path).unwrap();

            let status_cache_file = snapshot_dir.join(SNAPSHOT_STATUS_CACHE_FILENAME);
            File::create(status_cache_file).unwrap();

            let version_path = snapshot_dir.join(SNAPSHOT_VERSION_FILENAME);
            write_snapshot_version_file(version_path, SnapshotVersion::default()).unwrap();

            // Mark this directory complete so it can be used.  Check this flag first before selecting for deserialization.
            let state_complete_path = snapshot_dir.join(SNAPSHOT_STATE_COMPLETE_FILENAME);
            fs::File::create(state_complete_path).unwrap();
        }
    }

    #[test]
    fn test_get_bank_snapshots() {
        solana_logger::setup();
        let temp_snapshots_dir = tempfile::TempDir::new().unwrap();
        let min_slot = 10;
        let max_slot = 20;
        common_create_bank_snapshot_files(temp_snapshots_dir.path(), min_slot, max_slot);

        let bank_snapshots = get_bank_snapshots(temp_snapshots_dir.path());
        assert_eq!(bank_snapshots.len() as Slot, max_slot - min_slot);
    }

    #[test]
    fn test_get_highest_bank_snapshot_post() {
        solana_logger::setup();
        let temp_snapshots_dir = tempfile::TempDir::new().unwrap();
        let min_slot = 99;
        let max_slot = 123;
        common_create_bank_snapshot_files(temp_snapshots_dir.path(), min_slot, max_slot);

        let highest_bank_snapshot = get_highest_bank_snapshot_post(temp_snapshots_dir.path());
        assert!(highest_bank_snapshot.is_some());
        assert_eq!(highest_bank_snapshot.unwrap().slot, max_slot - 1);
    }

    /// A test helper function that creates full and incremental snapshot archive files.  Creates
    /// full snapshot files in the range (`min_full_snapshot_slot`, `max_full_snapshot_slot`], and
    /// incremental snapshot files in the range (`min_incremental_snapshot_slot`,
    /// `max_incremental_snapshot_slot`].  Additionally, "bad" files are created for both full and
    /// incremental snapshots to ensure the tests properly filter them out.
    fn common_create_snapshot_archive_files(
        full_snapshot_archives_dir: &Path,
        incremental_snapshot_archives_dir: &Path,
        min_full_snapshot_slot: Slot,
        max_full_snapshot_slot: Slot,
        min_incremental_snapshot_slot: Slot,
        max_incremental_snapshot_slot: Slot,
    ) {
        fs::create_dir_all(full_snapshot_archives_dir).unwrap();
        fs::create_dir_all(incremental_snapshot_archives_dir).unwrap();
        for full_snapshot_slot in min_full_snapshot_slot..max_full_snapshot_slot {
            for incremental_snapshot_slot in
                min_incremental_snapshot_slot..max_incremental_snapshot_slot
            {
                let snapshot_filename = format!(
                    "incremental-snapshot-{}-{}-{}.tar",
                    full_snapshot_slot,
                    incremental_snapshot_slot,
                    Hash::default()
                );
                let snapshot_filepath = incremental_snapshot_archives_dir.join(snapshot_filename);
                File::create(snapshot_filepath).unwrap();
            }

            let snapshot_filename =
                format!("snapshot-{}-{}.tar", full_snapshot_slot, Hash::default());
            let snapshot_filepath = full_snapshot_archives_dir.join(snapshot_filename);
            File::create(snapshot_filepath).unwrap();

            // Add in an incremental snapshot with a bad filename and high slot to ensure filename are filtered and sorted correctly
            let bad_filename = format!(
                "incremental-snapshot-{}-{}-bad!hash.tar",
                full_snapshot_slot,
                max_incremental_snapshot_slot + 1,
            );
            let bad_filepath = incremental_snapshot_archives_dir.join(bad_filename);
            File::create(bad_filepath).unwrap();
        }

        // Add in a snapshot with a bad filename and high slot to ensure filename are filtered and
        // sorted correctly
        let bad_filename = format!("snapshot-{}-bad!hash.tar", max_full_snapshot_slot + 1);
        let bad_filepath = full_snapshot_archives_dir.join(bad_filename);
        File::create(bad_filepath).unwrap();
    }

    #[test]
    fn test_get_full_snapshot_archives() {
        solana_logger::setup();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let min_slot = 123;
        let max_slot = 456;
        common_create_snapshot_archive_files(
            full_snapshot_archives_dir.path(),
            incremental_snapshot_archives_dir.path(),
            min_slot,
            max_slot,
            0,
            0,
        );

        let snapshot_archives = get_full_snapshot_archives(full_snapshot_archives_dir);
        assert_eq!(snapshot_archives.len() as Slot, max_slot - min_slot);
    }

    #[test]
    fn test_get_full_snapshot_archives_remote() {
        solana_logger::setup();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let min_slot = 123;
        let max_slot = 456;
        common_create_snapshot_archive_files(
            &full_snapshot_archives_dir
                .path()
                .join(SNAPSHOT_ARCHIVE_DOWNLOAD_DIR),
            &incremental_snapshot_archives_dir
                .path()
                .join(SNAPSHOT_ARCHIVE_DOWNLOAD_DIR),
            min_slot,
            max_slot,
            0,
            0,
        );

        let snapshot_archives = get_full_snapshot_archives(full_snapshot_archives_dir);
        assert_eq!(snapshot_archives.len() as Slot, max_slot - min_slot);
        assert!(snapshot_archives.iter().all(|info| info.is_remote()));
    }

    #[test]
    fn test_get_incremental_snapshot_archives() {
        solana_logger::setup();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let min_full_snapshot_slot = 12;
        let max_full_snapshot_slot = 23;
        let min_incremental_snapshot_slot = 34;
        let max_incremental_snapshot_slot = 45;
        common_create_snapshot_archive_files(
            full_snapshot_archives_dir.path(),
            incremental_snapshot_archives_dir.path(),
            min_full_snapshot_slot,
            max_full_snapshot_slot,
            min_incremental_snapshot_slot,
            max_incremental_snapshot_slot,
        );

        let incremental_snapshot_archives =
            get_incremental_snapshot_archives(incremental_snapshot_archives_dir);
        assert_eq!(
            incremental_snapshot_archives.len() as Slot,
            (max_full_snapshot_slot - min_full_snapshot_slot)
                * (max_incremental_snapshot_slot - min_incremental_snapshot_slot)
        );
    }

    #[test]
    fn test_get_incremental_snapshot_archives_remote() {
        solana_logger::setup();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let min_full_snapshot_slot = 12;
        let max_full_snapshot_slot = 23;
        let min_incremental_snapshot_slot = 34;
        let max_incremental_snapshot_slot = 45;
        common_create_snapshot_archive_files(
            &full_snapshot_archives_dir
                .path()
                .join(SNAPSHOT_ARCHIVE_DOWNLOAD_DIR),
            &incremental_snapshot_archives_dir
                .path()
                .join(SNAPSHOT_ARCHIVE_DOWNLOAD_DIR),
            min_full_snapshot_slot,
            max_full_snapshot_slot,
            min_incremental_snapshot_slot,
            max_incremental_snapshot_slot,
        );

        let incremental_snapshot_archives =
            get_incremental_snapshot_archives(incremental_snapshot_archives_dir);
        assert_eq!(
            incremental_snapshot_archives.len() as Slot,
            (max_full_snapshot_slot - min_full_snapshot_slot)
                * (max_incremental_snapshot_slot - min_incremental_snapshot_slot)
        );
        assert!(incremental_snapshot_archives
            .iter()
            .all(|info| info.is_remote()));
    }

    #[test]
    fn test_get_highest_full_snapshot_archive_slot() {
        solana_logger::setup();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let min_slot = 123;
        let max_slot = 456;
        common_create_snapshot_archive_files(
            full_snapshot_archives_dir.path(),
            incremental_snapshot_archives_dir.path(),
            min_slot,
            max_slot,
            0,
            0,
        );

        assert_eq!(
            get_highest_full_snapshot_archive_slot(full_snapshot_archives_dir.path()),
            Some(max_slot - 1)
        );
    }

    #[test]
    fn test_get_highest_incremental_snapshot_slot() {
        solana_logger::setup();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let min_full_snapshot_slot = 12;
        let max_full_snapshot_slot = 23;
        let min_incremental_snapshot_slot = 34;
        let max_incremental_snapshot_slot = 45;
        common_create_snapshot_archive_files(
            full_snapshot_archives_dir.path(),
            incremental_snapshot_archives_dir.path(),
            min_full_snapshot_slot,
            max_full_snapshot_slot,
            min_incremental_snapshot_slot,
            max_incremental_snapshot_slot,
        );

        for full_snapshot_slot in min_full_snapshot_slot..max_full_snapshot_slot {
            assert_eq!(
                get_highest_incremental_snapshot_archive_slot(
                    incremental_snapshot_archives_dir.path(),
                    full_snapshot_slot
                ),
                Some(max_incremental_snapshot_slot - 1)
            );
        }

        assert_eq!(
            get_highest_incremental_snapshot_archive_slot(
                incremental_snapshot_archives_dir.path(),
                max_full_snapshot_slot
            ),
            None
        );
    }

    fn common_test_purge_old_snapshot_archives(
        snapshot_names: &[&String],
        maximum_full_snapshot_archives_to_retain: NonZeroUsize,
        maximum_incremental_snapshot_archives_to_retain: NonZeroUsize,
        expected_snapshots: &[&String],
    ) {
        let temp_snap_dir = tempfile::TempDir::new().unwrap();

        for snap_name in snapshot_names {
            let snap_path = temp_snap_dir.path().join(snap_name);
            let mut _snap_file = File::create(snap_path);
        }
        purge_old_snapshot_archives(
            temp_snap_dir.path(),
            temp_snap_dir.path(),
            maximum_full_snapshot_archives_to_retain,
            maximum_incremental_snapshot_archives_to_retain,
        );

        let mut retained_snaps = HashSet::new();
        for entry in fs::read_dir(temp_snap_dir.path()).unwrap() {
            let entry_path_buf = entry.unwrap().path();
            let entry_path = entry_path_buf.as_path();
            let snapshot_name = entry_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();
            retained_snaps.insert(snapshot_name);
        }

        for snap_name in expected_snapshots {
            assert!(
                retained_snaps.contains(snap_name.as_str()),
                "{snap_name} not found"
            );
        }
        assert_eq!(retained_snaps.len(), expected_snapshots.len());
    }

    #[test]
    fn test_purge_old_full_snapshot_archives() {
        let snap1_name = format!("snapshot-1-{}.tar.zst", Hash::default());
        let snap2_name = format!("snapshot-3-{}.tar.zst", Hash::default());
        let snap3_name = format!("snapshot-50-{}.tar.zst", Hash::default());
        let snapshot_names = vec![&snap1_name, &snap2_name, &snap3_name];

        // expecting only the newest to be retained
        let expected_snapshots = vec![&snap3_name];
        common_test_purge_old_snapshot_archives(
            &snapshot_names,
            NonZeroUsize::new(1).unwrap(),
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            &expected_snapshots,
        );

        // retaining 2, expecting the 2 newest to be retained
        let expected_snapshots = vec![&snap2_name, &snap3_name];
        common_test_purge_old_snapshot_archives(
            &snapshot_names,
            NonZeroUsize::new(2).unwrap(),
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            &expected_snapshots,
        );

        // retaining 3, all three should be retained
        let expected_snapshots = vec![&snap1_name, &snap2_name, &snap3_name];
        common_test_purge_old_snapshot_archives(
            &snapshot_names,
            NonZeroUsize::new(3).unwrap(),
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            &expected_snapshots,
        );
    }

    /// Mimic a running node's behavior w.r.t. purging old snapshot archives.  Take snapshots in a
    /// loop, and periodically purge old snapshot archives.  After purging, check to make sure the
    /// snapshot archives on disk are correct.
    #[test]
    fn test_purge_old_full_snapshot_archives_in_the_loop() {
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let maximum_snapshots_to_retain = NonZeroUsize::new(5).unwrap();
        let starting_slot: Slot = 42;

        for slot in (starting_slot..).take(100) {
            let full_snapshot_archive_file_name =
                format!("snapshot-{}-{}.tar", slot, Hash::default());
            let full_snapshot_archive_path = full_snapshot_archives_dir
                .as_ref()
                .join(full_snapshot_archive_file_name);
            File::create(full_snapshot_archive_path).unwrap();

            // don't purge-and-check until enough snapshot archives have been created
            if slot < starting_slot + maximum_snapshots_to_retain.get() as Slot {
                continue;
            }

            // purge infrequently, so there will always be snapshot archives to purge
            if slot % (maximum_snapshots_to_retain.get() as Slot * 2) != 0 {
                continue;
            }

            purge_old_snapshot_archives(
                &full_snapshot_archives_dir,
                &incremental_snapshot_archives_dir,
                maximum_snapshots_to_retain,
                NonZeroUsize::new(usize::MAX).unwrap(),
            );
            let mut full_snapshot_archives =
                get_full_snapshot_archives(&full_snapshot_archives_dir);
            full_snapshot_archives.sort_unstable();
            assert_eq!(
                full_snapshot_archives.len(),
                maximum_snapshots_to_retain.get()
            );
            assert_eq!(full_snapshot_archives.last().unwrap().slot(), slot);
            for (i, full_snapshot_archive) in full_snapshot_archives.iter().rev().enumerate() {
                assert_eq!(full_snapshot_archive.slot(), slot - i as Slot);
            }
        }
    }

    #[test]
    fn test_purge_old_incremental_snapshot_archives() {
        solana_logger::setup();
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let starting_slot = 100_000;

        let maximum_incremental_snapshot_archives_to_retain =
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN;
        let maximum_full_snapshot_archives_to_retain = DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN;

        let incremental_snapshot_interval = 100;
        let num_incremental_snapshots_per_full_snapshot =
            maximum_incremental_snapshot_archives_to_retain.get() * 2;
        let full_snapshot_interval =
            incremental_snapshot_interval * num_incremental_snapshots_per_full_snapshot;

        let mut snapshot_filenames = vec![];
        (starting_slot..)
            .step_by(full_snapshot_interval)
            .take(
                maximum_full_snapshot_archives_to_retain
                    .checked_mul(NonZeroUsize::new(2).unwrap())
                    .unwrap()
                    .get(),
            )
            .for_each(|full_snapshot_slot| {
                let snapshot_filename =
                    format!("snapshot-{}-{}.tar", full_snapshot_slot, Hash::default());
                let snapshot_path = full_snapshot_archives_dir.path().join(&snapshot_filename);
                File::create(snapshot_path).unwrap();
                snapshot_filenames.push(snapshot_filename);

                (full_snapshot_slot..)
                    .step_by(incremental_snapshot_interval)
                    .take(num_incremental_snapshots_per_full_snapshot)
                    .skip(1)
                    .for_each(|incremental_snapshot_slot| {
                        let snapshot_filename = format!(
                            "incremental-snapshot-{}-{}-{}.tar",
                            full_snapshot_slot,
                            incremental_snapshot_slot,
                            Hash::default()
                        );
                        let snapshot_path = incremental_snapshot_archives_dir
                            .path()
                            .join(&snapshot_filename);
                        File::create(snapshot_path).unwrap();
                        snapshot_filenames.push(snapshot_filename);
                    });
            });

        purge_old_snapshot_archives(
            full_snapshot_archives_dir.path(),
            incremental_snapshot_archives_dir.path(),
            maximum_full_snapshot_archives_to_retain,
            maximum_incremental_snapshot_archives_to_retain,
        );

        // Ensure correct number of full snapshot archives are purged/retained
        let mut remaining_full_snapshot_archives =
            get_full_snapshot_archives(full_snapshot_archives_dir.path());
        assert_eq!(
            remaining_full_snapshot_archives.len(),
            maximum_full_snapshot_archives_to_retain.get(),
        );
        remaining_full_snapshot_archives.sort_unstable();
        let latest_full_snapshot_archive_slot =
            remaining_full_snapshot_archives.last().unwrap().slot();

        // Ensure correct number of incremental snapshot archives are purged/retained
        // For each additional full snapshot archive, one additional (the newest)
        // incremental snapshot archive is retained. This is accounted for by the
        // `+ maximum_full_snapshot_archives_to_retain.saturating_sub(1)`
        let mut remaining_incremental_snapshot_archives =
            get_incremental_snapshot_archives(incremental_snapshot_archives_dir.path());
        assert_eq!(
            remaining_incremental_snapshot_archives.len(),
            maximum_incremental_snapshot_archives_to_retain
                .get()
                .saturating_add(
                    maximum_full_snapshot_archives_to_retain
                        .get()
                        .saturating_sub(1)
                )
        );
        remaining_incremental_snapshot_archives.sort_unstable();
        remaining_incremental_snapshot_archives.reverse();

        // Ensure there exists one incremental snapshot all but the latest full snapshot
        for i in (1..maximum_full_snapshot_archives_to_retain.get()).rev() {
            let incremental_snapshot_archive =
                remaining_incremental_snapshot_archives.pop().unwrap();

            let expected_base_slot =
                latest_full_snapshot_archive_slot - (i * full_snapshot_interval) as u64;
            assert_eq!(incremental_snapshot_archive.base_slot(), expected_base_slot);
            let expected_slot = expected_base_slot
                + (full_snapshot_interval - incremental_snapshot_interval) as u64;
            assert_eq!(incremental_snapshot_archive.slot(), expected_slot);
        }

        // Ensure all remaining incremental snapshots are only for the latest full snapshot
        for incremental_snapshot_archive in &remaining_incremental_snapshot_archives {
            assert_eq!(
                incremental_snapshot_archive.base_slot(),
                latest_full_snapshot_archive_slot
            );
        }

        // Ensure the remaining incremental snapshots are at the right slot
        let expected_remaing_incremental_snapshot_archive_slots =
            (latest_full_snapshot_archive_slot..)
                .step_by(incremental_snapshot_interval)
                .take(num_incremental_snapshots_per_full_snapshot)
                .skip(
                    num_incremental_snapshots_per_full_snapshot
                        - maximum_incremental_snapshot_archives_to_retain.get(),
                )
                .collect::<HashSet<_>>();

        let actual_remaining_incremental_snapshot_archive_slots =
            remaining_incremental_snapshot_archives
                .iter()
                .map(|snapshot| snapshot.slot())
                .collect::<HashSet<_>>();
        assert_eq!(
            actual_remaining_incremental_snapshot_archive_slots,
            expected_remaing_incremental_snapshot_archive_slots
        );
    }

    #[test]
    fn test_purge_all_incremental_snapshot_archives_when_no_full_snapshot_archives() {
        let full_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let incremental_snapshot_archives_dir = tempfile::TempDir::new().unwrap();

        for snapshot_filenames in [
            format!("incremental-snapshot-100-120-{}.tar", Hash::default()),
            format!("incremental-snapshot-100-140-{}.tar", Hash::default()),
            format!("incremental-snapshot-100-160-{}.tar", Hash::default()),
            format!("incremental-snapshot-100-180-{}.tar", Hash::default()),
            format!("incremental-snapshot-200-220-{}.tar", Hash::default()),
            format!("incremental-snapshot-200-240-{}.tar", Hash::default()),
            format!("incremental-snapshot-200-260-{}.tar", Hash::default()),
            format!("incremental-snapshot-200-280-{}.tar", Hash::default()),
        ] {
            let snapshot_path = incremental_snapshot_archives_dir
                .path()
                .join(snapshot_filenames);
            File::create(snapshot_path).unwrap();
        }

        purge_old_snapshot_archives(
            full_snapshot_archives_dir.path(),
            incremental_snapshot_archives_dir.path(),
            NonZeroUsize::new(usize::MAX).unwrap(),
            NonZeroUsize::new(usize::MAX).unwrap(),
        );

        let remaining_incremental_snapshot_archives =
            get_incremental_snapshot_archives(incremental_snapshot_archives_dir.path());
        assert!(remaining_incremental_snapshot_archives.is_empty());
    }

    /// Test roundtrip of bank to a full snapshot, then back again.  This test creates the simplest
    /// bank possible, so the contents of the snapshot archive will be quite minimal.
    #[test]
    fn test_roundtrip_bank_to_and_from_full_snapshot_simple() {
        solana_logger::setup();
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
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
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
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            &Arc::default(),
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
        solana_logger::setup();
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
        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &collector, slot));
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
        let bank2 = Arc::new(Bank::new_from_parent(&bank1, &collector, slot));
        bank2
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        while !bank2.is_complete() {
            bank2.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank3 = Arc::new(Bank::new_from_parent(&bank2, &collector, slot));
        bank3
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        while !bank3.is_complete() {
            bank3.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank4 = Arc::new(Bank::new_from_parent(&bank3, &collector, slot));
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
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
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
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            &Arc::default(),
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
        solana_logger::setup();
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
        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &collector, slot));
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
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let slot = slot + 1;
        let bank2 = Arc::new(Bank::new_from_parent(&bank1, &collector, slot));
        bank2
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        while !bank2.is_complete() {
            bank2.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank3 = Arc::new(Bank::new_from_parent(&bank2, &collector, slot));
        bank3
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        while !bank3.is_complete() {
            bank3.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank4 = Arc::new(Bank::new_from_parent(&bank3, &collector, slot));
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
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
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
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            &Arc::default(),
        )
        .unwrap();
        roundtrip_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();
        assert_eq!(*bank4, roundtrip_bank);
    }

    /// Test rebuilding bank from the latest snapshot archives
    #[test]
    fn test_bank_from_latest_snapshot_archives() {
        solana_logger::setup();
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
        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &collector, slot));
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
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let slot = slot + 1;
        let bank2 = Arc::new(Bank::new_from_parent(&bank1, &collector, slot));
        bank2
            .transfer(sol_to_lamports(1.), &mint_keypair, &key1.pubkey())
            .unwrap();
        while !bank2.is_complete() {
            bank2.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank3 = Arc::new(Bank::new_from_parent(&bank2, &collector, slot));
        bank3
            .transfer(sol_to_lamports(2.), &mint_keypair, &key2.pubkey())
            .unwrap();
        while !bank3.is_complete() {
            bank3.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank4 = Arc::new(Bank::new_from_parent(&bank3, &collector, slot));
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
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
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
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            &Arc::default(),
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
        solana_logger::setup();

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
        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &collector, slot));
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
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let slot = slot + 1;
        let bank2 = Arc::new(Bank::new_from_parent(&bank1, &collector, slot));
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
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
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
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            &Arc::default(),
        )
        .unwrap();
        deserialized_bank.wait_for_initial_accounts_hash_verification_completed_for_tests();
        assert_eq!(
            deserialized_bank, *bank2,
            "Ensure rebuilding from an incremental snapshot works"
        );

        let slot = slot + 1;
        let bank3 = Arc::new(Bank::new_from_parent(&bank2, &collector, slot));
        // Update Account2 so that it no longer holds a reference to slot2
        bank3
            .transfer(lamports_to_transfer, &mint_keypair, &key2.pubkey())
            .unwrap();
        while !bank3.is_complete() {
            bank3.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank4 = Arc::new(Bank::new_from_parent(&bank3, &collector, slot));
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
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
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
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            &Arc::default(),
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
        solana_logger::setup();
        let collector = Pubkey::new_unique();
        let key1 = Keypair::new();

        let (genesis_config, mint_keypair) = create_genesis_config(sol_to_lamports(1_000_000.));
        let bank0 = Arc::new(Bank::new_for_tests(&genesis_config));
        while !bank0.is_complete() {
            bank0.register_tick(&Hash::new_unique());
        }

        let slot = 1;
        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &collector, slot));
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
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let slot = slot + 1;
        let bank2 = Arc::new(Bank::new_from_parent(&bank1, &collector, slot));
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
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        let bank_fields =
            bank_fields_from_snapshot_archives(&all_snapshots_dir, &all_snapshots_dir).unwrap();
        assert_eq!(bank_fields.slot, bank2.slot());
        assert_eq!(bank_fields.parent_slot, bank2.parent_slot());
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

    #[test]
    fn test_bank_snapshot_dir_accounts_hardlinks() {
        solana_logger::setup();
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
            .join(SNAPSHOT_ACCOUNTS_HARDLINKS);
        assert!(fs::metadata(&accounts_hardlinks_dir).is_ok());

        let mut hardlink_dirs: Vec<PathBuf> = Vec::new();
        // This directory contain symlinks to all accounts snapshot directories.
        for entry in fs::read_dir(accounts_hardlinks_dir).unwrap() {
            let entry = entry.unwrap();
            let symlink = entry.path();
            let dst_path = fs::read_link(symlink).unwrap();
            assert!(fs::metadata(&dst_path).is_ok());
            hardlink_dirs.push(dst_path);
        }

        let bank_snapshot_dir = get_bank_snapshot_dir(&bank_snapshots_dir, bank.slot());
        assert!(purge_bank_snapshot(bank_snapshot_dir).is_ok());

        // When the bank snapshot is removed, all the snapshot hardlink directories should be removed.
        assert!(hardlink_dirs.iter().all(|dir| fs::metadata(dir).is_err()));
    }

    #[test]
    fn test_get_snapshot_accounts_hardlink_dir() {
        solana_logger::setup();

        let slot: Slot = 1;

        let mut account_paths_set: HashSet<PathBuf> = HashSet::new();

        let bank_snapshots_dir_tmp = tempfile::TempDir::new().unwrap();
        let bank_snapshot_dir = bank_snapshots_dir_tmp.path().join(slot.to_string());
        let accounts_hardlinks_dir = bank_snapshot_dir.join(SNAPSHOT_ACCOUNTS_HARDLINKS);
        fs::create_dir_all(&accounts_hardlinks_dir).unwrap();

        let (_tmp_dir, accounts_dir) = create_tmp_accounts_dir_for_tests();
        let appendvec_filename = format!("{slot}.0");
        let appendvec_path = accounts_dir.join(appendvec_filename);

        let ret = get_snapshot_accounts_hardlink_dir(
            &appendvec_path,
            slot,
            &mut account_paths_set,
            &accounts_hardlinks_dir,
        );
        assert!(ret.is_ok());

        let wrong_appendvec_path = appendvec_path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(appendvec_path.file_name().unwrap());
        let ret = get_snapshot_accounts_hardlink_dir(
            &wrong_appendvec_path,
            slot,
            &mut account_paths_set,
            accounts_hardlinks_dir,
        );

        assert!(matches!(ret, Err(SnapshotError::InvalidAppendVecPath(_))));
    }

    #[test]
    fn test_get_highest_bank_snapshot() {
        solana_logger::setup();

        let genesis_config = GenesisConfig::default();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let _bank = create_snapshot_dirs_for_tests(&genesis_config, &bank_snapshots_dir, 4, 0);

        let snapshot = get_highest_bank_snapshot(&bank_snapshots_dir).unwrap();
        assert_eq!(snapshot.slot, 4);

        let complete_flag_file = snapshot.snapshot_dir.join(SNAPSHOT_STATE_COMPLETE_FILENAME);
        fs::remove_file(complete_flag_file).unwrap();
        // The incomplete snapshot dir should still exist
        let snapshot_dir_4 = snapshot.snapshot_dir;
        assert!(snapshot_dir_4.exists());
        let snapshot = get_highest_bank_snapshot(&bank_snapshots_dir).unwrap();
        assert_eq!(snapshot.slot, 3);
        // The incomplete snapshot dir should have been deleted
        assert!(!snapshot_dir_4.exists());

        let snapshot_version_file = snapshot.snapshot_dir.join(SNAPSHOT_VERSION_FILENAME);
        fs::remove_file(snapshot_version_file).unwrap();
        let snapshot = get_highest_bank_snapshot(&bank_snapshots_dir).unwrap();
        assert_eq!(snapshot.slot, 2);

        let status_cache_file = snapshot.snapshot_dir.join(SNAPSHOT_STATUS_CACHE_FILENAME);
        fs::remove_file(status_cache_file).unwrap();
        let snapshot = get_highest_bank_snapshot(&bank_snapshots_dir).unwrap();
        assert_eq!(snapshot.slot, 1);
    }

    #[test]
    pub fn test_create_all_accounts_run_and_snapshot_dirs() {
        solana_logger::setup();

        let (_tmp_dirs, account_paths): (Vec<TempDir>, Vec<PathBuf>) = (0..4)
            .map(|_| {
                let tmp_dir = tempfile::TempDir::new().unwrap();
                let account_path = tmp_dir.path().join("accounts");
                (tmp_dir, account_path)
            })
            .unzip();

        // Set the parent directory of the first account path to be readonly, so that
        // create_dir_all in create_all_accounts_run_and_snapshot_dirs fails.
        let account_path_first = &account_paths[0];
        let parent = account_path_first.parent().unwrap();
        let mut parent_permissions = fs::metadata(parent).unwrap().permissions();
        parent_permissions.set_readonly(true);
        fs::set_permissions(parent, parent_permissions.clone()).unwrap();

        // assert that create_all_accounts_run_and_snapshot_dirs returns error when the first account path
        // is readonly.
        assert!(create_all_accounts_run_and_snapshot_dirs(&account_paths).is_err());

        // Set the parent directory of the first account path to be writable, so that
        // create_all_accounts_run_and_snapshot_dirs returns Ok.
        parent_permissions.set_mode(0o744);
        fs::set_permissions(parent, parent_permissions.clone()).unwrap();
        let result = create_all_accounts_run_and_snapshot_dirs(&account_paths);
        assert!(result.is_ok());

        let (account_run_paths, account_snapshot_paths) = result.unwrap();
        account_run_paths.iter().all(|path| path.is_dir());
        account_snapshot_paths.iter().all(|path| path.is_dir());

        delete_contents_of_path(account_path_first);
        assert!(account_path_first.exists());
        let mut permissions = fs::metadata(account_path_first).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(account_path_first, permissions.clone()).unwrap();
        parent_permissions.set_readonly(true);
        fs::set_permissions(parent, parent_permissions.clone()).unwrap();
        // assert that create_all_accounts_run_and_snapshot_dirs returns error when the first account path
        // and its parent are readonly.  This exercises the case where the first account path is readonly,
        // causing create_accounts_run_and_snapshot_dirs to fail.
        assert!(create_all_accounts_run_and_snapshot_dirs(&account_paths).is_err());
    }

    #[test]
    fn test_clean_orphaned_account_snapshot_dirs() {
        solana_logger::setup();

        let genesis_config = GenesisConfig::default();
        let bank_snapshots_dir = tempfile::TempDir::new().unwrap();
        let _bank = create_snapshot_dirs_for_tests(&genesis_config, &bank_snapshots_dir, 2, 0);

        let snapshot_dir_slot_2 = bank_snapshots_dir.path().join("2");
        let accounts_link_dir_slot_2 = snapshot_dir_slot_2.join(SNAPSHOT_ACCOUNTS_HARDLINKS);

        // the symlinks point to the account snapshot hardlink directories <account_path>/snapshot/<slot>/ for slot 2
        // get them via read_link
        let hardlink_dirs_slot_2: Vec<PathBuf> = fs::read_dir(accounts_link_dir_slot_2)
            .unwrap()
            .map(|entry| {
                let symlink = entry.unwrap().path();
                fs::read_link(symlink).unwrap()
            })
            .collect();

        // remove the bank snapshot directory for slot 2, so the account snapshot slot 2 directories become orphaned
        fs::remove_dir_all(snapshot_dir_slot_2).unwrap();

        // verify the orphaned account snapshot hardlink directories are still there
        assert!(hardlink_dirs_slot_2
            .iter()
            .all(|dir| fs::metadata(dir).is_ok()));

        let account_snapshot_paths: Vec<PathBuf> = hardlink_dirs_slot_2
            .iter()
            .map(|dir| dir.parent().unwrap().parent().unwrap().to_path_buf())
            .collect();
        // clean the orphaned hardlink directories
        clean_orphaned_account_snapshot_dirs(&bank_snapshots_dir, &account_snapshot_paths).unwrap();

        // verify the hardlink directories are gone
        assert!(hardlink_dirs_slot_2
            .iter()
            .all(|dir| fs::metadata(dir).is_err()));
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
            bank = Arc::new(Bank::new_from_parent(
                &bank,
                &Pubkey::new_unique(),
                bank.slot() + 1,
            ));
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
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
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
            bank = Arc::new(Bank::new_from_parent(
                &bank,
                &Pubkey::new_unique(),
                bank.slot() + 1,
            ));
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
            DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
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
            Some(ACCOUNTS_DB_CONFIG_FOR_TESTING),
            None,
            &Arc::default(),
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
                },
                &SortedStorages::new(&other_incremental_snapshot_storages),
                HashStats::default(),
            )
            .unwrap();
        assert_eq!(other_incremental_accounts_hash, incremental_accounts_hash);
    }

    #[test]
    fn test_bank_from_snapshot_dir() {
        solana_logger::setup();

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
            &Arc::default(),
        )
        .unwrap();

        bank_constructed.wait_for_initial_accounts_hash_verification_completed_for_tests();
        assert_eq!(bank_constructed, bank);

        // Verify that the next_append_vec_id tracking is correct
        let mut max_id = 0;
        for path in account_paths {
            fs::read_dir(path).unwrap().for_each(|entry| {
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
        solana_logger::setup();

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
            &Arc::default(),
        )
        .unwrap();

        assert_eq!(
            deserialized_bank, bank,
            "Ensure rebuilding bank from the highest snapshot dir results in the highest bank",
        );
    }

    #[test]
    fn test_purge_old_bank_snapshots() {
        solana_logger::setup();

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
}

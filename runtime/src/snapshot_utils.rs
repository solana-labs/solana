use {
    crate::{
        serde_snapshot::SnapshotStreams,
        snapshot_archive_info::{
            FullSnapshotArchiveInfo, IncrementalSnapshotArchiveInfo, SnapshotArchiveInfoGetter,
        },
        snapshot_hash::SnapshotHash,
        snapshot_package::SnapshotPackage,
        snapshot_utils::snapshot_storage_rebuilder::{
            RebuiltSnapshotStorage, SnapshotStorageRebuilder,
        },
    },
    bzip2::bufread::BzDecoder,
    crossbeam_channel::Sender,
    flate2::read::GzDecoder,
    fs_err,
    lazy_static::lazy_static,
    log::*,
    regex::Regex,
    solana_accounts_db::{
        account_storage::AccountStorageMap,
        accounts_db::{
            self, create_accounts_run_and_snapshot_dirs, AccountStorageEntry, AtomicAppendVecId,
        },
        accounts_file::AccountsFileError,
        append_vec::AppendVec,
        hardened_unpack::{self, ParallelSelector, UnpackError},
        shared_buffer_reader::{SharedBuffer, SharedBufferReader},
    },
    solana_measure::{measure, measure::Measure},
    solana_sdk::{clock::Slot, hash::Hash},
    std::{
        cmp::Ordering,
        collections::{HashMap, HashSet},
        fmt,
        io::{BufReader, BufWriter, Error as IoError, ErrorKind, Read, Seek, Write},
        num::NonZeroUsize,
        path::{Path, PathBuf},
        process::ExitStatus,
        str::FromStr,
        sync::{atomic::AtomicU32, Arc, Mutex},
        thread::{Builder, JoinHandle},
    },
    tar::{self, Archive},
    tempfile::TempDir,
    thiserror::Error,
};
#[cfg(feature = "dev-context-only-utils")]
use {hardened_unpack::UnpackedAppendVecMap, rayon::prelude::*};

mod archive_format;
pub mod snapshot_storage_rebuilder;
pub use archive_format::*;

pub const SNAPSHOT_STATUS_CACHE_FILENAME: &str = "status_cache";
pub const SNAPSHOT_VERSION_FILENAME: &str = "version";
pub const SNAPSHOT_STATE_COMPLETE_FILENAME: &str = "state_complete";
pub const SNAPSHOT_ACCOUNTS_HARDLINKS: &str = "accounts_hardlinks";
pub const SNAPSHOT_ARCHIVE_DOWNLOAD_DIR: &str = "remote";
pub const MAX_SNAPSHOT_DATA_FILE_SIZE: u64 = 32 * 1024 * 1024 * 1024; // 32 GiB
const MAX_SNAPSHOT_VERSION_FILE_SIZE: u64 = 8; // byte
const VERSION_STRING_V1_2_0: &str = "1.2.0";
pub const TMP_SNAPSHOT_ARCHIVE_PREFIX: &str = "tmp-snapshot-archive-";
pub const BANK_SNAPSHOT_PRE_FILENAME_EXTENSION: &str = "pre";
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
        if !is_bank_snapshot_complete(&bank_snapshot_dir) {
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

        // NOTE: It is important that checking for "Pre" happens before "Post.
        //
        // Consider the scenario where AccountsHashVerifier is actively processing an
        // AccountsPackage for a snapshot/slot; if AHV is in the middle of reserializing the
        // bank snapshot file (writing the new "Post" file), and then the process dies,
        // there will be an incomplete "Post" file on disk.  We do not want only the existence of
        // this "Post" file to be sufficient for deciding the snapshot type as "Post".  More so,
        // "Post" *requires* the *absence* of a "Pre" file.
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
pub struct SnapshotRootPaths {
    pub full_snapshot_root_file_path: PathBuf,
    pub incremental_snapshot_root_file_path: Option<PathBuf>,
}

/// Helper type to bundle up the results from `unarchive_snapshot()`
#[derive(Debug)]
pub struct UnarchivedSnapshot {
    #[allow(dead_code)]
    unpack_dir: TempDir,
    pub storage: AccountStorageMap,
    pub unpacked_snapshots_dir_and_version: UnpackedSnapshotsDirAndVersion,
    pub measure_untar: Measure,
}

/// Helper type for passing around the unpacked snapshots dir and the snapshot version together
#[derive(Debug)]
pub struct UnpackedSnapshotsDirAndVersion {
    pub unpacked_snapshots_dir: PathBuf,
    pub snapshot_version: SnapshotVersion,
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

    #[error("AccountsFile error: {0}")]
    AccountsFileError(#[from] AccountsFileError),

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

    #[error("could not get file name from path: {0}")]
    PathToFileNameError(PathBuf),

    #[error("could not get str from file name: {0}")]
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

    #[error("invalid snapshot dir path: {0}")]
    InvalidSnapshotDirPath(PathBuf),

    #[error("invalid AppendVec path: {0}")]
    InvalidAppendVecPath(PathBuf),

    #[error("invalid account path: {0}")]
    InvalidAccountPath(PathBuf),

    #[error("no valid snapshot dir found under {0}")]
    NoSnapshotSlotDir(PathBuf),

    #[error("snapshot dir account paths mismatching")]
    AccountPathsMismatch,

    #[error("failed to add bank snapshot for slot {1}: {0}")]
    AddBankSnapshot(#[source] AddBankSnapshotError, Slot),
}

#[derive(Error, Debug)]
pub enum SnapshotNewFromDirError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid bank snapshot directory {0}")]
    InvalidBankSnapshotDir(PathBuf),

    #[error("missing status cache file {0}")]
    MissingStatusCacheFile(PathBuf),

    #[error("missing version file {0}")]
    MissingVersionFile(PathBuf),

    #[error("invalid snapshot version")]
    InvalidVersion,

    #[error("snapshot directory incomplete {0}")]
    IncompleteDir(PathBuf),

    #[error("missing snapshot file {0}")]
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

/// Errors that can happen in `add_bank_snapshot()`
#[derive(Error, Debug)]
pub enum AddBankSnapshotError {
    #[error("bank snapshot dir already exists: {0}")]
    SnapshotDirAlreadyExists(PathBuf),

    #[error("failed to create snapshot dir: {0}")]
    CreateSnapshotDir(#[source] std::io::Error),

    #[error("failed to hard link storages: {0}")]
    HardLinkStorages(#[source] HardLinkStoragesToSnapshotError),

    #[error("failed to serialize bank: {0}")]
    SerializeBank(#[source] Box<SnapshotError>),

    #[error("failed to serialize status cache: {0}")]
    SerializeStatusCache(#[source] Box<SnapshotError>),

    #[error("failed to write snapshot version file: {0}")]
    WriteSnapshotVersionFile(#[source] std::io::Error),

    #[error("failed to mark snapshot as 'complete': {0}")]
    CreateStateCompleteFile(#[source] std::io::Error),
}

/// Errors that can happen in `hard_link_storages_to_snapshot()`
#[derive(Error, Debug)]
pub enum HardLinkStoragesToSnapshotError {
    #[error("failed to create accounts hard links dir: {0}")]
    CreateAccountsHardLinksDir(#[source] std::io::Error),

    #[error("failed to flush storage: {0}")]
    FlushStorage(#[source] AccountsFileError),

    #[error("failed to get the snapshot's accounts hard link dir: {0}")]
    GetSnapshotHardLinksDir(#[from] GetSnapshotAccountsHardLinkDirError),

    #[error("failed to hard link storage: {0}")]
    HardLinkStorage(#[source] std::io::Error),
}

/// Errors that can happen in `get_snapshot_accounts_hardlink_dir()`
#[derive(Error, Debug)]
pub enum GetSnapshotAccountsHardLinkDirError {
    #[error("invalid account storage path: {0}")]
    GetAccountPath(PathBuf),

    #[error("failed to create the snapshot hard link dir: {0}")]
    CreateSnapshotHardLinkDir(#[source] std::io::Error),

    #[error("failed to symlink snapshot hard link dir {link} to {original}: {source}")]
    SymlinkSnapshotHardLinkDir {
        source: std::io::Error,
        original: PathBuf,
        link: PathBuf,
    },
}

/// Creates directories if they do not exist, and canonicalizes the paths.
pub fn create_and_canonicalize_directories(directories: &[PathBuf]) -> Result<Vec<PathBuf>> {
    directories
        .iter()
        .map(|path| {
            fs_err::create_dir_all(path)?;
            let path = fs_err::canonicalize(path)?;
            Ok(path)
        })
        .collect()
}

/// Delete the files and subdirectories in a directory.
/// This is useful if the process does not have permission
/// to delete the top level directory it might be able to
/// delete the contents of that directory.
pub(crate) fn delete_contents_of_path(path: impl AsRef<Path>) {
    accounts_db::delete_contents_of_path(path)
}

/// Moves and asynchronously deletes the contents of a directory to avoid blocking on it.
/// The directory is re-created after the move, and should now be empty.
pub fn move_and_async_delete_path_contents(path: impl AsRef<Path>) {
    move_and_async_delete_path(&path);
    // The following could fail if the rename failed.
    // If that happens, the directory should be left as is.
    // So we ignore errors here.
    _ = std::fs::create_dir(path);
}

/// Delete directories/files asynchronously to avoid blocking on it.
/// First, in sync context, check if the original path exists, if it
/// does, rename the original path to *_to_be_deleted.
/// If there's an in-progress deleting thread for this path, return.
/// Then spawn a thread to delete the renamed path.
pub fn move_and_async_delete_path(path: impl AsRef<Path>) {
    lazy_static! {
        static ref IN_PROGRESS_DELETES: Mutex<HashSet<PathBuf>> = Mutex::new(HashSet::new());
    };

    // Grab the mutex so no new async delete threads can be spawned for this path.
    let mut lock = IN_PROGRESS_DELETES.lock().unwrap();

    // If the path does not exist, there's nothing to delete.
    if !path.as_ref().exists() {
        return;
    }

    // If the original path (`pathbuf` here) is already being deleted,
    // then the path should not be moved and deleted again.
    if lock.contains(path.as_ref()) {
        return;
    }

    let mut path_delete = path.as_ref().to_path_buf();
    path_delete.set_file_name(format!(
        "{}{}",
        path_delete.file_name().unwrap().to_str().unwrap(),
        "_to_be_deleted"
    ));
    if let Err(err) = fs_err::rename(&path, &path_delete) {
        warn!("Path renaming failed, falling back to rm_dir in sync mode: {err}");
        // Although the delete here is synchronous, we want to prevent another thread
        // from moving & deleting this directory via `move_and_async_delete_path`.
        lock.insert(path.as_ref().to_path_buf());
        drop(lock); // unlock before doing sync delete

        delete_contents_of_path(&path);
        IN_PROGRESS_DELETES.lock().unwrap().remove(path.as_ref());
        return;
    }

    lock.insert(path_delete.clone());
    drop(lock);
    Builder::new()
        .name("solDeletePath".to_string())
        .spawn(move || {
            trace!("background deleting {}...", path_delete.display());
            let (_, measure_delete) =
                measure!(fs_err::remove_dir_all(&path_delete).expect("background delete"));
            trace!(
                "background deleting {}... Done, and{measure_delete}",
                path_delete.display()
            );

            IN_PROGRESS_DELETES.lock().unwrap().remove(&path_delete);
        })
        .expect("spawn background delete thread");
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
        for entry in fs_err::read_dir(&account_hardlinks_dir)? {
            let path = entry?.path();
            let target = fs_err::read_link(&path)?;
            account_snapshot_dirs_referenced.insert(target);
        }
    }

    // loop through the account snapshot hardlink directories, if the directory is not in the account_snapshot_dirs_referenced set, delete it
    for account_snapshot_path in account_snapshot_paths {
        for entry in fs_err::read_dir(account_snapshot_path)? {
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

/// Purges incomplete bank snapshots
pub fn purge_incomplete_bank_snapshots(bank_snapshots_dir: impl AsRef<Path>) {
    let Ok(read_dir_iter) = std::fs::read_dir(&bank_snapshots_dir) else {
        // If we cannot read the bank snapshots dir, then there's nothing to do
        return;
    };

    let is_incomplete = |dir: &PathBuf| !is_bank_snapshot_complete(dir);

    let incomplete_dirs: Vec<_> = read_dir_iter
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(is_incomplete)
        .collect();

    // attempt to purge all the incomplete directories; do not exit early
    for incomplete_dir in incomplete_dirs {
        let result = purge_bank_snapshot(&incomplete_dir);
        match result {
            Ok(_) => info!(
                "Purged incomplete snapshot dir: {}",
                incomplete_dir.display()
            ),
            Err(err) => warn!("Failed to purge incomplete snapshot dir: {err}"),
        }
    }
}

/// Is the bank snapshot complete?
fn is_bank_snapshot_complete(bank_snapshot_dir: impl AsRef<Path>) -> bool {
    let state_complete_path = bank_snapshot_dir
        .as_ref()
        .join(SNAPSHOT_STATE_COMPLETE_FILENAME);
    state_complete_path.is_file()
}

/// If the validator halts in the middle of `archive_snapshot_package()`, the temporary staging
/// directory won't be cleaned up.  Call this function to clean them up.
pub fn remove_tmp_snapshot_archives(snapshot_archives_dir: impl AsRef<Path>) {
    if let Ok(entries) = std::fs::read_dir(snapshot_archives_dir) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_str()
                .map(|file_name| file_name.starts_with(TMP_SNAPSHOT_ARCHIVE_PREFIX))
                .unwrap_or(false)
            {
                let path = entry.path();
                let result = if path.is_dir() {
                    fs_err::remove_dir_all(path)
                } else {
                    fs_err::remove_file(path)
                };
                if let Err(err) = result {
                    warn!("Failed to remove temporary snapshot archive: {err}");
                }
            }
        }
    }
}

/// Write the snapshot version as a file into the bank snapshot directory
pub fn write_snapshot_version_file(
    version_file: impl AsRef<Path>,
    version: SnapshotVersion,
) -> std::io::Result<()> {
    fs_err::write(version_file, version.as_str().as_bytes())
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

    fs_err::create_dir_all(tar_dir)
        .map_err(|err| SnapshotError::IoWithSource(err, "create archive path"))?;

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
    fs_err::create_dir_all(&staging_accounts_dir)
        .map_err(|err| SnapshotError::IoWithSource(err, "create staging accounts path"))?;

    let slot_str = snapshot_package.slot().to_string();
    let staging_snapshot_dir = staging_snapshots_dir.join(&slot_str);
    // Creates staging snapshots/<slot>/
    fs_err::create_dir_all(&staging_snapshot_dir)
        .map_err(|err| SnapshotError::IoWithSource(err, "create staging snapshots path"))?;

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
        let output_path = staging_accounts_dir.join(AppendVec::file_name(
            storage.slot(),
            storage.append_vec_id(),
        ));

        // `storage_path` - The file path where the AppendVec itself is located
        // `output_path` - The file path where the AppendVec will be placed in the staging directory.
        let storage_path =
            fs_err::canonicalize(storage_path).expect("Could not get absolute path for accounts");
        symlink::symlink_file(storage_path, &output_path)
            .map_err(|e| SnapshotError::IoWithSource(e, "create storage symlink"))?;
        if !output_path.is_file() {
            return Err(SnapshotError::StoragePathSymlinkInvalid);
        }
    }

    write_snapshot_version_file(staging_version_file, snapshot_package.snapshot_version)
        .map_err(|err| SnapshotError::IoWithSource(err, "write snapshot version file"))?;

    // Tar the staging directory into the archive at `archive_path`
    let archive_path = tar_dir.join(format!(
        "{}{}.{}",
        staging_dir_prefix,
        snapshot_package.slot(),
        snapshot_package.archive_format().extension(),
    ));

    {
        let mut archive_file = fs_err::File::create(&archive_path)?;

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
    let metadata = fs_err::metadata(&archive_path)
        .map_err(|err| SnapshotError::IoWithSource(err, "archive path stat"))?;
    fs_err::rename(&archive_path, snapshot_package.path())
        .map_err(|err| SnapshotError::IoWithSource(err, "archive path rename"))?;

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
            if snapshot_package.snapshot_kind.is_full_snapshot() {
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
    match fs_err::read_dir(bank_snapshots_dir.as_ref()) {
        Err(err) => {
            info!("Unable to read bank snapshots directory: {err}");
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
                    Ok(snapshot_info) => bank_snapshots.push(snapshot_info),
                    // Other threads may be modifying bank snapshots in parallel; only return
                    // snapshots that are complete as deemed by BankSnapshotInfo::new_from_dir()
                    Err(err) => debug!("Unable to read bank snapshot for slot {slot}: {err}"),
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
    bank_snapshots.into_iter().next_back()
}

pub fn serialize_snapshot_data_file<F>(data_file_path: &Path, serializer: F) -> Result<u64>
where
    F: FnOnce(&mut BufWriter<std::fs::File>) -> Result<()>,
{
    serialize_snapshot_data_file_capped::<F>(
        data_file_path,
        MAX_SNAPSHOT_DATA_FILE_SIZE,
        serializer,
    )
}

pub fn deserialize_snapshot_data_file<T: Sized>(
    data_file_path: &Path,
    deserializer: impl FnOnce(&mut BufReader<std::fs::File>) -> Result<T>,
) -> Result<T> {
    let wrapped_deserializer = move |streams: &mut SnapshotStreams<std::fs::File>| -> Result<T> {
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

pub fn deserialize_snapshot_data_files<T: Sized>(
    snapshot_root_paths: &SnapshotRootPaths,
    deserializer: impl FnOnce(&mut SnapshotStreams<std::fs::File>) -> Result<T>,
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
    F: FnOnce(&mut BufWriter<std::fs::File>) -> Result<()>,
{
    let data_file = fs_err::File::create(data_file_path)?.into();
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
    deserializer: impl FnOnce(&mut SnapshotStreams<std::fs::File>) -> Result<T>,
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
            Some(create_snapshot_data_file_stream(
                incremental_snapshot_root_file_path,
                maximum_file_size,
            )?)
        } else {
            None
        }
        .unzip();

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
) -> Result<(u64, BufReader<std::fs::File>)> {
    let snapshot_file_size = fs_err::metadata(&snapshot_root_file_path)?.len();

    if snapshot_file_size > maximum_file_size {
        let error_message = format!(
            "too large snapshot data file to deserialize: {} has {} bytes (max size is {} bytes)",
            snapshot_root_file_path.as_ref().display(),
            snapshot_file_size,
            maximum_file_size,
        );
        return Err(get_io_error(&error_message));
    }

    let snapshot_data_file = fs_err::File::open(snapshot_root_file_path.as_ref())?;
    let snapshot_data_file_stream = BufReader::new(snapshot_data_file.into());

    Ok((snapshot_file_size, snapshot_data_file_stream))
}

/// After running the deserializer function, perform common checks to ensure the snapshot archive
/// files were consumed correctly.
fn check_deserialize_file_consumed(
    file_size: u64,
    file_path: impl AsRef<Path>,
    file_stream: &mut BufReader<std::fs::File>,
) -> Result<()> {
    let consumed_size = file_stream.stream_position()?;

    if consumed_size != file_size {
        let error_message = format!(
            "invalid snapshot data file: {} has {} bytes, however consumed {} bytes to deserialize",
            file_path.as_ref().display(),
            file_size,
            consumed_size,
        );
        return Err(get_io_error(&error_message));
    }

    Ok(())
}

/// For all account_paths, create the run/ and snapshot/ sub directories.
/// If an account_path directory does not exist, create it.
/// It returns (account_run_paths, account_snapshot_paths) or error
pub fn create_all_accounts_run_and_snapshot_dirs(
    account_paths: &[PathBuf],
) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    accounts_db::create_all_accounts_run_and_snapshot_dirs(account_paths).map_err(|err| {
        SnapshotError::IoWithSource(err, "Unable to create account run and snapshot directories")
    })
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
) -> std::result::Result<PathBuf, GetSnapshotAccountsHardLinkDirError> {
    let account_path = get_account_path_from_appendvec_path(appendvec_path).ok_or_else(|| {
        GetSnapshotAccountsHardLinkDirError::GetAccountPath(appendvec_path.to_path_buf())
    })?;

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
        fs_err::create_dir_all(&snapshot_hardlink_dir)
            .map_err(GetSnapshotAccountsHardLinkDirError::CreateSnapshotHardLinkDir)?;
        let symlink_path = hardlinks_dir.as_ref().join(format!("account_path_{idx}"));
        symlink::symlink_dir(&snapshot_hardlink_dir, &symlink_path).map_err(|err| {
            GetSnapshotAccountsHardLinkDirError::SymlinkSnapshotHardLinkDir {
                source: err,
                original: snapshot_hardlink_dir.clone(),
                link: symlink_path,
            }
        })?;
        account_paths.insert(account_path);
    };

    Ok(snapshot_hardlink_dir)
}

/// Hard-link the files from accounts/ to snapshot/<bank_slot>/accounts/
/// This keeps the appendvec files alive and with the bank snapshot.  The slot and id
/// in the file names are also updated in case its file is a recycled one with inconsistent slot
/// and id.
pub fn hard_link_storages_to_snapshot(
    bank_snapshot_dir: impl AsRef<Path>,
    bank_slot: Slot,
    snapshot_storages: &[Arc<AccountStorageEntry>],
) -> std::result::Result<(), HardLinkStoragesToSnapshotError> {
    let accounts_hardlinks_dir = bank_snapshot_dir.as_ref().join(SNAPSHOT_ACCOUNTS_HARDLINKS);
    fs_err::create_dir_all(&accounts_hardlinks_dir)
        .map_err(HardLinkStoragesToSnapshotError::CreateAccountsHardLinksDir)?;

    let mut account_paths: HashSet<PathBuf> = HashSet::new();
    for storage in snapshot_storages {
        storage
            .flush()
            .map_err(HardLinkStoragesToSnapshotError::FlushStorage)?;
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
        fs_err::hard_link(&storage_path, &hard_link_path)
            .map_err(HardLinkStoragesToSnapshotError::HardLinkStorage)?;
    }
    Ok(())
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

pub fn verify_and_unarchive_snapshots(
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
            hardened_unpack::streaming_unpack_snapshot(
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
    let slot_dir = std::fs::read_dir(&snapshots_dir)
        .map_err(|_| SnapshotError::NoSnapshotSlotDir(snapshots_dir.clone()))?
        .find(|entry| entry.as_ref().unwrap().path().is_dir())
        .ok_or_else(|| SnapshotError::NoSnapshotSlotDir(snapshots_dir.clone()))?
        .map_err(|_| SnapshotError::NoSnapshotSlotDir(snapshots_dir.clone()))?
        .path();

    let version_file = unpack_dir.as_ref().join(SNAPSHOT_VERSION_FILENAME);
    fs_err::hard_link(version_file, slot_dir.join(SNAPSHOT_VERSION_FILENAME))?;

    let status_cache_file = snapshots_dir.join(SNAPSHOT_STATUS_CACHE_FILENAME);
    fs_err::hard_link(
        status_cache_file,
        slot_dir.join(SNAPSHOT_STATUS_CACHE_FILENAME),
    )?;

    let state_complete_file = slot_dir.join(SNAPSHOT_STATE_COMPLETE_FILENAME);
    fs_err::File::create(state_complete_file)?;

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
        for file in fs_err::read_dir(account_path)? {
            file_sender.send(file?.path())?;
        }
    }

    Ok(())
}

/// Perform the common tasks when deserialize a snapshot.  Handles reading snapshot file, reading the version file,
/// and then returning those fields plus the rebuilt storage
pub fn build_storage_from_snapshot_dir(
    snapshot_info: &BankSnapshotInfo,
    account_paths: &[PathBuf],
    next_append_vec_id: Arc<AtomicAppendVecId>,
) -> Result<AccountStorageMap> {
    let bank_snapshot_dir = &snapshot_info.snapshot_dir;
    let accounts_hardlinks = bank_snapshot_dir.join(SNAPSHOT_ACCOUNTS_HARDLINKS);
    let account_run_paths: HashSet<_> = HashSet::from_iter(account_paths);

    for dir_entry in fs_err::read_dir(accounts_hardlinks)? {
        let symlink_path = dir_entry?.path();
        // The symlink point to <account_path>/snapshot/<slot> which contain the account files hardlinks
        // The corresponding run path should be <account_path>/run/
        let account_snapshot_path = fs_err::read_link(&symlink_path)?;
        let account_run_path = account_snapshot_path
            .parent()
            .ok_or_else(|| SnapshotError::InvalidAccountPath(account_snapshot_path.clone()))?
            .parent()
            .ok_or_else(|| SnapshotError::InvalidAccountPath(account_snapshot_path.clone()))?
            .join("run");
        if !account_run_paths.contains(&account_run_path) {
            // The appendvec from the bank snapshot storage does not match any of the provided account_paths set.
            // The accout paths have changed so the snapshot is no longer usable.
            return Err(SnapshotError::AccountPathsMismatch);
        }
        // Generate hard-links to make the account files available in the main accounts/, and let the new appendvec
        // paths be in accounts/
        for file in fs_err::read_dir(&account_snapshot_path)? {
            let file_path = file?.path();
            let file_name = file_path
                .file_name()
                .ok_or_else(|| SnapshotError::InvalidAppendVecPath(file_path.to_path_buf()))?;
            let dest_path = account_run_path.join(file_name);
            fs_err::hard_link(&file_path, &dest_path)?;
        }
    }

    let (file_sender, file_receiver) = crossbeam_channel::unbounded();
    let snapshot_file_path = &snapshot_info.snapshot_path();
    let snapshot_version_path = bank_snapshot_dir.join(SNAPSHOT_VERSION_FILENAME);
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
    let file_size = fs_err::metadata(&path)?.len();
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
    fs_err::File::open(path.as_ref()).and_then(|mut f| f.read_to_string(&mut snapshot_version))?;
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
        let entry_iter = fs_err::read_dir(dir);
        match entry_iter {
            Err(err) => {
                info!("Unable to read snapshot archives directory: {err}");
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
    full_snapshot_archives.into_iter().next_back()
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
    incremental_snapshot_archives.into_iter().next_back()
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
            let result = fs_err::remove_file(path);
            if let Err(err) = result {
                info!("Failed to remove snapshot archive: {err}",);
            }
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

#[cfg(feature = "dev-context-only-utils")]
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
            hardened_unpack::unpack_snapshot(
                &mut archive,
                ledger_dir,
                account_paths,
                parallel_selector,
            )
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
    let open_file = || fs_err::File::open(snapshot_tar).unwrap();
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

#[cfg(feature = "dev-context-only-utils")]
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

pub fn verify_unpacked_snapshots_dir_and_version(
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

#[cfg(feature = "dev-context-only-utils")]
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
    fs_err::create_dir_all(&storages_to_verify).unwrap();

    let slot = slot.to_string();
    let snapshot_slot_dir = snapshots_to_verify.as_ref().join(&slot);

    if let VerifyBank::NonDeterministic = verify_bank {
        // file contents may be different, but deserialized structs should be equal
        let p1 = snapshots_to_verify.as_ref().join(&slot).join(&slot);
        let p2 = unpacked_snapshots.join(&slot).join(&slot);
        assert!(crate::serde_snapshot::compare_two_serialized_banks(&p1, &p2).unwrap());
        fs_err::remove_file(p1).unwrap();
        fs_err::remove_file(p2).unwrap();
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
    fs_err::rename(
        existing_unpacked_status_cache_file,
        new_unpacked_status_cache_file,
    )
    .unwrap();

    let accounts_hardlinks_dir = snapshot_slot_dir.join(SNAPSHOT_ACCOUNTS_HARDLINKS);
    if accounts_hardlinks_dir.is_dir() {
        // This directory contain symlinks to all <account_path>/snapshot/<slot> directories.
        for entry in fs_err::read_dir(&accounts_hardlinks_dir).unwrap() {
            let link_dst_path = fs_err::read_link(entry.unwrap().path()).unwrap();
            // Copy all the files in dst_path into the storages_to_verify directory.
            for entry in fs_err::read_dir(&link_dst_path).unwrap() {
                let src_path = entry.unwrap().path();
                let dst_path = storages_to_verify.join(src_path.file_name().unwrap());
                fs_err::copy(src_path, dst_path).unwrap();
            }
        }
        fs_err::remove_dir_all(accounts_hardlinks_dir).unwrap();
    }

    let version_path = snapshot_slot_dir.join(SNAPSHOT_VERSION_FILENAME);
    if version_path.is_file() {
        fs_err::remove_file(version_path).unwrap();
    }

    let state_complete_path = snapshot_slot_dir.join(SNAPSHOT_STATE_COMPLETE_FILENAME);
    if state_complete_path.is_file() {
        fs_err::remove_file(state_complete_path).unwrap();
    }

    assert!(!dir_diff::is_different(&snapshots_to_verify, unpacked_snapshots).unwrap());

    // In the unarchiving case, there is an extra empty "accounts" directory. The account
    // files in the archive accounts/ have been expanded to [account_paths].
    // Remove the empty "accounts" directory for the directory comparison below.
    // In some test cases the directory to compare do not come from unarchiving.
    // Ignore the error when this directory does not exist.
    _ = fs_err::remove_dir(unpack_account_dir.join("accounts"));
    // Check the account entries are the same
    assert!(!dir_diff::is_different(&storages_to_verify, unpack_account_dir).unwrap());
}

/// Purges bank snapshots, retaining the newest `num_bank_snapshots_to_retain`
pub fn purge_old_bank_snapshots(
    bank_snapshots_dir: impl AsRef<Path>,
    num_bank_snapshots_to_retain: usize,
    filter_by_type: Option<BankSnapshotType>,
) {
    let mut bank_snapshots = match filter_by_type {
        Some(BankSnapshotType::Pre) => get_bank_snapshots_pre(&bank_snapshots_dir),
        Some(BankSnapshotType::Post) => get_bank_snapshots_post(&bank_snapshots_dir),
        None => get_bank_snapshots(&bank_snapshots_dir),
    };

    bank_snapshots.sort_unstable();
    purge_bank_snapshots(
        bank_snapshots
            .iter()
            .rev()
            .skip(num_bank_snapshots_to_retain),
    );
}

/// At startup, purge old (i.e. unusable) bank snapshots
///
/// Only a single bank snapshot could be needed at startup (when using fast boot), so
/// retain the highest bank snapshot "post", and purge the rest.
pub fn purge_old_bank_snapshots_at_startup(bank_snapshots_dir: impl AsRef<Path>) {
    purge_old_bank_snapshots(&bank_snapshots_dir, 0, Some(BankSnapshotType::Pre));
    purge_old_bank_snapshots(&bank_snapshots_dir, 1, Some(BankSnapshotType::Post));

    let highest_bank_snapshot_post = get_highest_bank_snapshot_post(&bank_snapshots_dir);
    if let Some(highest_bank_snapshot_post) = highest_bank_snapshot_post {
        debug!(
            "Retained bank snapshot for slot {}, and purged the rest.",
            highest_bank_snapshot_post.slot
        );
    }
}

/// Purges bank snapshots that are older than `slot`
pub fn purge_bank_snapshots_older_than_slot(bank_snapshots_dir: impl AsRef<Path>, slot: Slot) {
    let mut bank_snapshots = get_bank_snapshots(&bank_snapshots_dir);
    bank_snapshots.retain(|bank_snapshot| bank_snapshot.slot < slot);
    purge_bank_snapshots(&bank_snapshots);
}

/// Purges all `bank_snapshots`
///
/// Does not exit early if there is an error while purging a bank snapshot.
fn purge_bank_snapshots<'a>(bank_snapshots: impl IntoIterator<Item = &'a BankSnapshotInfo>) {
    for snapshot_dir in bank_snapshots.into_iter().map(|s| &s.snapshot_dir) {
        if purge_bank_snapshot(snapshot_dir).is_err() {
            warn!("Failed to purge bank snapshot: {}", snapshot_dir.display());
        }
    }
}

/// Remove the bank snapshot at this path
pub fn purge_bank_snapshot(bank_snapshot_dir: impl AsRef<Path>) -> Result<()> {
    let accounts_hardlinks_dir = bank_snapshot_dir.as_ref().join(SNAPSHOT_ACCOUNTS_HARDLINKS);
    if accounts_hardlinks_dir.is_dir() {
        // This directory contain symlinks to all accounts snapshot directories.
        // They should all be removed.
        for accounts_hardlink_dir in fs_err::read_dir(accounts_hardlinks_dir)? {
            let accounts_hardlink_dir = fs_err::read_link(accounts_hardlink_dir?.path())?;
            move_and_async_delete_path(&accounts_hardlink_dir);
        }
    }
    fs_err::remove_dir_all(bank_snapshot_dir)?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use {
        super::*,
        assert_matches::assert_matches,
        bincode::{deserialize_from, serialize_into},
        std::{convert::TryFrom, mem::size_of},
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
            fs_err::create_dir_all(&snapshot_dir).unwrap();

            let snapshot_filename = get_snapshot_file_name(slot);
            let snapshot_path = snapshot_dir.join(snapshot_filename);
            fs_err::File::create(snapshot_path).unwrap();

            let status_cache_file = snapshot_dir.join(SNAPSHOT_STATUS_CACHE_FILENAME);
            fs_err::File::create(status_cache_file).unwrap();

            let version_path = snapshot_dir.join(SNAPSHOT_VERSION_FILENAME);
            write_snapshot_version_file(version_path, SnapshotVersion::default()).unwrap();

            // Mark this directory complete so it can be used.  Check this flag first before selecting for deserialization.
            let state_complete_path = snapshot_dir.join(SNAPSHOT_STATE_COMPLETE_FILENAME);
            fs_err::File::create(state_complete_path).unwrap();
        }
    }

    #[test]
    fn test_get_bank_snapshots() {
        let temp_snapshots_dir = tempfile::TempDir::new().unwrap();
        let min_slot = 10;
        let max_slot = 20;
        common_create_bank_snapshot_files(temp_snapshots_dir.path(), min_slot, max_slot);

        let bank_snapshots = get_bank_snapshots(temp_snapshots_dir.path());
        assert_eq!(bank_snapshots.len() as Slot, max_slot - min_slot);
    }

    #[test]
    fn test_get_highest_bank_snapshot_post() {
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
        fs_err::create_dir_all(full_snapshot_archives_dir).unwrap();
        fs_err::create_dir_all(incremental_snapshot_archives_dir).unwrap();
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
                fs_err::File::create(snapshot_filepath).unwrap();
            }

            let snapshot_filename =
                format!("snapshot-{}-{}.tar", full_snapshot_slot, Hash::default());
            let snapshot_filepath = full_snapshot_archives_dir.join(snapshot_filename);
            fs_err::File::create(snapshot_filepath).unwrap();

            // Add in an incremental snapshot with a bad filename and high slot to ensure filename are filtered and sorted correctly
            let bad_filename = format!(
                "incremental-snapshot-{}-{}-bad!hash.tar",
                full_snapshot_slot,
                max_incremental_snapshot_slot + 1,
            );
            let bad_filepath = incremental_snapshot_archives_dir.join(bad_filename);
            fs_err::File::create(bad_filepath).unwrap();
        }

        // Add in a snapshot with a bad filename and high slot to ensure filename are filtered and
        // sorted correctly
        let bad_filename = format!("snapshot-{}-bad!hash.tar", max_full_snapshot_slot + 1);
        let bad_filepath = full_snapshot_archives_dir.join(bad_filename);
        fs_err::File::create(bad_filepath).unwrap();
    }

    #[test]
    fn test_get_full_snapshot_archives() {
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
            let mut _snap_file = fs_err::File::create(snap_path);
        }
        purge_old_snapshot_archives(
            temp_snap_dir.path(),
            temp_snap_dir.path(),
            maximum_full_snapshot_archives_to_retain,
            maximum_incremental_snapshot_archives_to_retain,
        );

        let mut retained_snaps = HashSet::new();
        for entry in fs_err::read_dir(temp_snap_dir.path()).unwrap() {
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
            fs_err::File::create(full_snapshot_archive_path).unwrap();

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
                fs_err::File::create(snapshot_path).unwrap();
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
                        fs_err::File::create(snapshot_path).unwrap();
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
            fs_err::File::create(snapshot_path).unwrap();
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

    #[test]
    fn test_get_snapshot_accounts_hardlink_dir() {
        let slot: Slot = 1;

        let mut account_paths_set: HashSet<PathBuf> = HashSet::new();

        let bank_snapshots_dir_tmp = tempfile::TempDir::new().unwrap();
        let bank_snapshot_dir = bank_snapshots_dir_tmp.path().join(slot.to_string());
        let accounts_hardlinks_dir = bank_snapshot_dir.join(SNAPSHOT_ACCOUNTS_HARDLINKS);
        fs_err::create_dir_all(&accounts_hardlinks_dir).unwrap();

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

        assert_matches!(
            ret,
            Err(GetSnapshotAccountsHardLinkDirError::GetAccountPath(_))
        );
    }
}

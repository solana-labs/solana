use {
    crate::{
        accounts_db::{AccountShrinkThreshold, AccountsDb},
        accounts_index::AccountSecondaryIndexes,
        bank::{Bank, BankSlotDelta, Builtins},
        bank_forks::ArchiveFormat,
        hardened_unpack::{unpack_snapshot, UnpackError, UnpackedAppendVecMap},
        serde_snapshot::{
            bank_from_stream, bank_from_stream_incremental, bank_to_stream, SerdeStyle,
            SnapshotStorage, SnapshotStorages,
        },
        snapshot_package::{
            AccountsPackage, AccountsPackagePre, AccountsPackageSendError, AccountsPackageSender,
        },
        sorted_storages::SortedStorages,
    },
    bincode::{config::Options, serialize_into},
    bzip2::bufread::BzDecoder,
    flate2::read::GzDecoder,
    log::*,
    rayon::ThreadPool,
    regex::Regex,
    solana_measure::measure::Measure,
    solana_sdk::{clock::Slot, genesis_config::GenesisConfig, hash::Hash, pubkey::Pubkey},
    std::{
        cmp::max,
        cmp::Ordering,
        collections::HashSet,
        fmt,
        fs::{self, File},
        io::{
            self, BufReader, BufWriter, Error as IoError, ErrorKind, Read, Seek, SeekFrom, Write,
        },
        path::{Path, PathBuf},
        process::{self, ExitStatus},
        str::FromStr,
        sync::Arc,
    },
    tar::Archive,
    thiserror::Error,
};

/// Information about a snapshot archive: its path, slot, hash, and archive format
pub type SnapshotArchiveInfo = (PathBuf, (Slot, Hash, ArchiveFormat));

/// Information about an incremental snapshot archive: its path, full snapshot base slot, incremental snapshot slot, hash, and archive format
pub type IncrementalSnapshotArchiveInfo = (PathBuf, (Slot, Slot, Hash, ArchiveFormat));

pub const SNAPSHOT_STATUS_CACHE_FILE_NAME: &str = "status_cache";

pub const MAX_SNAPSHOTS: usize = 8; // Save some snapshots but not too many
const MAX_SNAPSHOT_DATA_FILE_SIZE: u64 = 32 * 1024 * 1024 * 1024; // 32 GiB
const VERSION_STRING_V1_2_0: &str = "1.2.0";
const DEFAULT_SNAPSHOT_VERSION: SnapshotVersion = SnapshotVersion::V1_2_0;
const TMP_SNAPSHOT_PREFIX: &str = "tmp-snapshot-";
const TMP_INCREMENTAL_SNAPSHOT_PREFIX: &str = "tmp-incremental-snapshot-";
pub const DEFAULT_MAX_SNAPSHOTS_TO_RETAIN: usize = 2;

pub const SNAPSHOT_ARCHIVE_FILENAME_REGEX: &str =
    r"^snapshot-(\d+)-([[:alnum:]]+)\.(tar|tar\.bz2|tar\.zst|tar\.gz)$";

pub const INCREMENTAL_SNAPSHOT_ARCHIVE_FILENAME_REGEX: &str =
    r"^incremental-snapshot-(\d+)-(\d+)-([[:alnum:]]+)\.(tar|tar\.bz2|tar\.zst|tar\.gz)$";

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum SnapshotVersion {
    V1_2_0,
}

impl Default for SnapshotVersion {
    fn default() -> Self {
        DEFAULT_SNAPSHOT_VERSION
    }
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

    fn maybe_from_string(version_string: &str) -> Option<SnapshotVersion> {
        version_string.parse::<Self>().ok()
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct SlotSnapshotPaths {
    pub slot: Slot,
    pub snapshot_file_path: PathBuf,
}

#[derive(Error, Debug)]
pub enum SnapshotError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialize(#[from] bincode::Error),

    #[error("archive generation failure {0}")]
    ArchiveGenerationFailure(ExitStatus),

    #[error("storage path symlink is invalid")]
    StoragePathSymlinkInvalid,

    #[error("Unpack error: {0}")]
    UnpackError(#[from] UnpackError),

    #[error("accounts package send error")]
    AccountsPackageSendError(#[from] AccountsPackageSendError),

    #[error("Parsing path error: {0}")]
    PathParseError(&'static str),

    #[error("Incompatible snapshots error: full snapshot slot ({0}) and incremental snapshot base slot ({1}) do not match!")]
    IncompatibleSnapshots(Slot, Slot),
}
pub type Result<T> = std::result::Result<T, SnapshotError>;

impl PartialOrd for SlotSnapshotPaths {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.slot.cmp(&other.slot))
    }
}

impl Ord for SlotSnapshotPaths {
    fn cmp(&self, other: &Self) -> Ordering {
        self.slot.cmp(&other.slot)
    }
}

pub fn package_snapshot<P: AsRef<Path>, Q: AsRef<Path>>(
    bank: &Bank,
    snapshot_files: &SlotSnapshotPaths,
    snapshot_path: Q,
    status_cache_slot_deltas: Vec<BankSlotDelta>,
    snapshot_package_output_path: P,
    snapshot_storages: SnapshotStorages,
    archive_format: ArchiveFormat,
    snapshot_version: SnapshotVersion,
    hash_for_testing: Option<Hash>,
) -> Result<AccountsPackagePre> {
    // Hard link all the snapshots we need for this package
    let snapshot_tmpdir = tempfile::Builder::new()
        .prefix(&format!("{}{}-", TMP_SNAPSHOT_PREFIX, bank.slot()))
        .tempdir_in(snapshot_path)?;

    // Create a snapshot package
    info!(
        "Snapshot for bank: {} has {} account storage entries",
        bank.slot(),
        snapshot_storages.len()
    );

    // Hard link the snapshot into a tmpdir, to ensure its not removed prior to packaging.
    {
        let snapshot_hardlink_dir = snapshot_tmpdir
            .as_ref()
            .join(snapshot_files.slot.to_string());
        fs::create_dir_all(&snapshot_hardlink_dir)?;
        fs::hard_link(
            &snapshot_files.snapshot_file_path,
            &snapshot_hardlink_dir.join(snapshot_files.slot.to_string()),
        )?;
    }

    let package = AccountsPackagePre::new(
        bank.slot(),
        bank.block_height(),
        status_cache_slot_deltas,
        snapshot_tmpdir,
        snapshot_storages,
        bank.get_accounts_hash(),
        archive_format,
        snapshot_version,
        snapshot_package_output_path.as_ref().to_path_buf(),
        bank.capitalization(),
        hash_for_testing,
        bank.cluster_type(),
    );

    Ok(package)
}

#[allow(clippy::too_many_arguments)]
fn package_incremental_snapshot<P: AsRef<Path>, Q: AsRef<Path>>(
    bank: &Bank,
    full_snapshot_slot: Slot,
    snapshot_files: &SlotSnapshotPaths,
    snapshot_path: Q,
    status_cache_slot_deltas: Vec<BankSlotDelta>,
    snapshot_package_output_path: P,
    snapshot_storages: SnapshotStorages,
    archive_format: ArchiveFormat,
    snapshot_version: SnapshotVersion,
    hash_for_testing: Option<Hash>,
) -> Result<AccountsPackagePre> {
    // Hard link all the snapshots we need for this package
    let snapshot_tmpdir = tempfile::Builder::new()
        .prefix(&format!(
            "{}{}-{}-",
            TMP_INCREMENTAL_SNAPSHOT_PREFIX,
            full_snapshot_slot,
            bank.slot()
        ))
        .tempdir_in(snapshot_path)?;

    // Create an incremental snapshot package
    info!(
        "Incremental snapshot for bank {} (from base {}) has {} account storage entries",
        bank.slot(),
        full_snapshot_slot,
        snapshot_storages.len()
    );

    // Hard link the snapshot into a tmpdir, to ensure its not removed prior to packaging.
    {
        let snapshot_hardlink_dir = snapshot_tmpdir
            .as_ref()
            .join(snapshot_files.slot.to_string());
        fs::create_dir_all(&snapshot_hardlink_dir)?;
        fs::hard_link(
            &snapshot_files.snapshot_file_path,
            &snapshot_hardlink_dir.join(snapshot_files.slot.to_string()),
        )?;
    }

    let package = AccountsPackagePre::new(
        bank.slot(),
        bank.block_height(),
        status_cache_slot_deltas,
        snapshot_tmpdir,
        snapshot_storages,
        bank.get_accounts_hash(),
        archive_format,
        snapshot_version,
        snapshot_package_output_path.as_ref().to_path_buf(),
        bank.capitalization(),
        hash_for_testing,
        bank.cluster_type(),
    );

    Ok(package)
}

fn get_archive_ext(archive_format: ArchiveFormat) -> &'static str {
    match archive_format {
        ArchiveFormat::TarBzip2 => "tar.bz2",
        ArchiveFormat::TarGzip => "tar.gz",
        ArchiveFormat::TarZstd => "tar.zst",
        ArchiveFormat::Tar => "tar",
    }
}

// If the validator is halted in the middle of `archive_snapshot_package` the temporary staging directory
// won't be cleaned up.  Call this function to clean them up
pub fn remove_tmp_snapshot_archives(snapshot_path: &Path) {
    if let Ok(entries) = fs::read_dir(&snapshot_path) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            if entry
                .file_name()
                .into_string()
                .unwrap_or_else(|_| String::new())
                .starts_with(TMP_SNAPSHOT_PREFIX)
            {
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

pub fn archive_snapshot_package(
    snapshot_package: &AccountsPackage,
    maximum_snapshots_to_retain: usize,
) -> Result<()> {
    info!(
        "Generating snapshot archive for slot {}",
        snapshot_package.slot
    );

    serialize_status_cache(
        snapshot_package.slot,
        &snapshot_package.slot_deltas,
        &snapshot_package
            .snapshot_links
            .path()
            .join(SNAPSHOT_STATUS_CACHE_FILE_NAME),
    )?;

    let mut timer = Measure::start("snapshot_package-package_snapshots");
    let tar_dir = snapshot_package
        .tar_output_file
        .parent()
        .expect("Tar output path is invalid");

    fs::create_dir_all(tar_dir)?;

    // Create the staging directories
    let staging_dir = tempfile::Builder::new()
        .prefix(&format!(
            "{}{}-",
            TMP_SNAPSHOT_PREFIX, snapshot_package.slot
        ))
        .tempdir_in(tar_dir)?;

    let staging_accounts_dir = staging_dir.path().join("accounts");
    let staging_snapshots_dir = staging_dir.path().join("snapshots");
    let staging_version_file = staging_dir.path().join("version");
    fs::create_dir_all(&staging_accounts_dir)?;

    // Add the snapshots to the staging directory
    symlink::symlink_dir(
        snapshot_package.snapshot_links.path(),
        &staging_snapshots_dir,
    )?;

    // Add the AppendVecs into the compressible list
    for storage in snapshot_package.storages.iter().flatten() {
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
        symlink::symlink_file(storage_path, &output_path)?;
        if !output_path.is_file() {
            return Err(SnapshotError::StoragePathSymlinkInvalid);
        }
    }

    // Write version file
    {
        let mut f = fs::File::create(staging_version_file)?;
        f.write_all(snapshot_package.snapshot_version.as_str().as_bytes())?;
    }

    let file_ext = get_archive_ext(snapshot_package.archive_format);

    // Tar the staging directory into the archive at `archive_path`
    //
    // system `tar` program is used for -S (sparse file support)
    let archive_path = tar_dir.join(format!(
        "{}{}.{}",
        TMP_SNAPSHOT_PREFIX, snapshot_package.slot, file_ext
    ));

    let mut tar = process::Command::new("tar")
        .args(&[
            "chS",
            "-C",
            staging_dir.path().to_str().unwrap(),
            "accounts",
            "snapshots",
            "version",
        ])
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::inherit())
        .spawn()?;

    match &mut tar.stdout {
        None => {
            return Err(SnapshotError::Io(IoError::new(
                ErrorKind::Other,
                "tar stdout unavailable".to_string(),
            )));
        }
        Some(tar_output) => {
            let mut archive_file = fs::File::create(&archive_path)?;

            match snapshot_package.archive_format {
                ArchiveFormat::TarBzip2 => {
                    let mut encoder =
                        bzip2::write::BzEncoder::new(archive_file, bzip2::Compression::Best);
                    io::copy(tar_output, &mut encoder)?;
                    let _ = encoder.finish()?;
                }
                ArchiveFormat::TarGzip => {
                    let mut encoder =
                        flate2::write::GzEncoder::new(archive_file, flate2::Compression::default());
                    io::copy(tar_output, &mut encoder)?;
                    let _ = encoder.finish()?;
                }
                ArchiveFormat::Tar => {
                    io::copy(tar_output, &mut archive_file)?;
                }
                ArchiveFormat::TarZstd => {
                    let mut encoder = zstd::stream::Encoder::new(archive_file, 0)?;
                    io::copy(tar_output, &mut encoder)?;
                    let _ = encoder.finish()?;
                }
            };
        }
    }

    let tar_exit_status = tar.wait()?;
    if !tar_exit_status.success() {
        warn!("tar command failed with exit code: {}", tar_exit_status);
        return Err(SnapshotError::ArchiveGenerationFailure(tar_exit_status));
    }

    // Atomically move the archive into position for other validators to find
    let metadata = fs::metadata(&archive_path)?;
    fs::rename(&archive_path, &snapshot_package.tar_output_file)?;

    // bprumo TODO: Here is a place that we could set AccountsDb::last_full_snapshot_slot.  At this
    // point we would know definitively that the full snapshot was successfully created.  However,
    // there is no bank or reference back to AccountsDb...

    purge_old_snapshot_archives(
        snapshot_package.tar_output_file.parent().unwrap(),
        maximum_snapshots_to_retain,
    );

    timer.stop();
    info!(
        "Successfully created {:?}. slot: {}, elapsed ms: {}, size={}",
        snapshot_package.tar_output_file,
        snapshot_package.slot,
        timer.as_ms(),
        metadata.len()
    );
    datapoint_info!(
        "snapshot-package",
        ("slot", snapshot_package.slot, i64),
        ("duration_ms", timer.as_ms(), i64),
        ("size", metadata.len(), i64)
    );
    Ok(())
}

pub fn get_snapshot_paths<P: AsRef<Path>>(snapshot_path: P) -> Vec<SlotSnapshotPaths>
where
    P: fmt::Debug,
{
    match fs::read_dir(&snapshot_path) {
        Ok(paths) => {
            let mut names = paths
                .filter_map(|entry| {
                    entry.ok().and_then(|e| {
                        e.path()
                            .file_name()
                            .and_then(|n| n.to_str().map(|s| s.parse::<u64>().ok()))
                            .unwrap_or(None)
                    })
                })
                .map(|slot| {
                    let snapshot_path = snapshot_path.as_ref().join(slot.to_string());
                    SlotSnapshotPaths {
                        slot,
                        snapshot_file_path: snapshot_path.join(get_snapshot_file_name(slot)),
                    }
                })
                .collect::<Vec<SlotSnapshotPaths>>();

            names.sort();
            names
        }
        Err(err) => {
            info!(
                "Unable to read snapshot directory {:?}: {}",
                snapshot_path, err
            );
            vec![]
        }
    }
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

pub fn deserialize_snapshot_data_file<F, T>(data_file_path: &Path, deserializer: F) -> Result<T>
where
    F: FnOnce(&mut BufReader<File>) -> Result<T>,
{
    deserialize_snapshot_data_file_capped::<F, T>(
        data_file_path,
        MAX_SNAPSHOT_DATA_FILE_SIZE,
        deserializer,
    )
}

pub fn deserialize_incremental_snapshot_data_file<F, T>(
    data_file_path1: &Path,
    data_file_path2: &Path,
    deserializer: F,
) -> Result<T>
where
    F: FnOnce(&mut BufReader<File>, &mut BufReader<File>) -> Result<T>,
{
    deserialize_incremental_snapshot_data_file_capped::<F, T>(
        data_file_path1,
        data_file_path2,
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

    let consumed_size = data_file_stream.seek(SeekFrom::Current(0))?;
    if consumed_size > maximum_file_size {
        let error_message = format!(
            "too large snapshot data file to serialize: {:?} has {} bytes",
            data_file_path, consumed_size
        );
        return Err(get_io_error(&error_message));
    }
    Ok(consumed_size)
}

fn deserialize_snapshot_data_file_capped<F, T>(
    data_file_path: &Path,
    maximum_file_size: u64,
    deserializer: F,
) -> Result<T>
where
    F: FnOnce(&mut BufReader<File>) -> Result<T>,
{
    let file_size = fs::metadata(&data_file_path)?.len();

    if file_size > maximum_file_size {
        let error_message = format!(
            "too large snapshot data file to deserialize: {:?} has {} bytes",
            data_file_path, file_size
        );
        return Err(get_io_error(&error_message));
    }

    let data_file = File::open(data_file_path)?;
    let mut data_file_stream = BufReader::new(data_file);

    let ret = deserializer(&mut data_file_stream)?;

    let consumed_size = data_file_stream.seek(SeekFrom::Current(0))?;

    if file_size != consumed_size {
        let error_message = format!(
            "invalid snapshot data file: {:?} has {} bytes, however consumed {} bytes to deserialize",
            data_file_path, file_size, consumed_size
        );
        return Err(get_io_error(&error_message));
    }

    Ok(ret)
}

fn deserialize_incremental_snapshot_data_file_capped<F, T>(
    fss_data_file_path: &Path,
    iss_data_file_path: &Path,
    maximum_file_size: u64,
    deserializer: F,
) -> Result<T>
where
    F: FnOnce(&mut BufReader<File>, &mut BufReader<File>) -> Result<T>,
{
    let fss_file_size = fs::metadata(&fss_data_file_path)?.len();
    if fss_file_size > maximum_file_size {
        let error_message = format!(
            "too large snapshot data file to deserialize: {:?} has {} bytes",
            fss_data_file_path, fss_file_size
        );
        return Err(get_io_error(&error_message));
    }

    let iss_file_size = fs::metadata(&iss_data_file_path)?.len();
    if iss_file_size > maximum_file_size {
        let error_message = format!(
            "too large snapshot data file to deserialize: {:?} has {} bytes",
            iss_data_file_path, iss_file_size
        );
        return Err(get_io_error(&error_message));
    }

    let fss_data_file = File::open(fss_data_file_path)?;
    let mut fss_data_file_stream = BufReader::new(fss_data_file);

    let iss_data_file = File::open(iss_data_file_path)?;
    let mut iss_data_file_stream = BufReader::new(iss_data_file);

    let ret = deserializer(&mut fss_data_file_stream, &mut iss_data_file_stream)?;

    let fss_consumed_size = fss_data_file_stream.seek(SeekFrom::Current(0))?;
    if fss_file_size != fss_consumed_size {
        let error_message = format!(
            "invalid snapshot data file: {:?} has {} bytes, however consumed {} bytes to deserialize",
            fss_data_file_path, fss_file_size, fss_consumed_size
        );
        return Err(get_io_error(&error_message));
    }

    let iss_consumed_size = iss_data_file_stream.seek(SeekFrom::Current(0))?;
    if iss_file_size != iss_consumed_size {
        let error_message = format!(
            "invalid snapshot data file: {:?} has {} bytes, however consumed {} bytes to deserialize",
            iss_data_file_path, iss_file_size, iss_consumed_size
        );
        return Err(get_io_error(&error_message));
    }

    Ok(ret)
}

pub fn add_snapshot<P: AsRef<Path>>(
    snapshot_path: P,
    bank: &Bank,
    snapshot_storages: &[SnapshotStorage],
    snapshot_version: SnapshotVersion,
) -> Result<SlotSnapshotPaths> {
    let slot = bank.slot();
    // snapshot_path/slot
    let slot_snapshot_dir = get_bank_snapshot_dir(snapshot_path, slot);
    fs::create_dir_all(slot_snapshot_dir.clone())?;

    // the bank snapshot is stored as snapshot_path/slot/slot
    let snapshot_bank_file_path = slot_snapshot_dir.join(get_snapshot_file_name(slot));
    info!(
        "Creating snapshot for slot {}, path: {:?}",
        slot, snapshot_bank_file_path,
    );

    let mut bank_serialize = Measure::start("bank-serialize-ms");
    let bank_snapshot_serializer = move |stream: &mut BufWriter<File>| -> Result<()> {
        let serde_style = match snapshot_version {
            SnapshotVersion::V1_2_0 => SerdeStyle::Newer,
        };
        bank_to_stream(serde_style, stream.by_ref(), bank, snapshot_storages)?;
        Ok(())
    };
    let consumed_size =
        serialize_snapshot_data_file(&snapshot_bank_file_path, bank_snapshot_serializer)?;
    bank_serialize.stop();

    // Monitor sizes because they're capped to MAX_SNAPSHOT_DATA_FILE_SIZE
    datapoint_info!(
        "snapshot-bank-file",
        ("slot", slot, i64),
        ("size", consumed_size, i64)
    );

    inc_new_counter_info!("bank-serialize-ms", bank_serialize.as_ms() as usize);

    info!(
        "{} for slot {} at {:?}",
        bank_serialize, slot, snapshot_bank_file_path,
    );

    Ok(SlotSnapshotPaths {
        slot,
        snapshot_file_path: snapshot_bank_file_path,
    })
}

fn serialize_status_cache(
    slot: Slot,
    slot_deltas: &[BankSlotDelta],
    status_cache_path: &Path,
) -> Result<()> {
    let mut status_cache_serialize = Measure::start("status_cache_serialize-ms");
    let consumed_size = serialize_snapshot_data_file(status_cache_path, |stream| {
        serialize_into(stream, slot_deltas)?;
        Ok(())
    })?;
    status_cache_serialize.stop();

    // Monitor sizes because they're capped to MAX_SNAPSHOT_DATA_FILE_SIZE
    datapoint_info!(
        "snapshot-status-cache-file",
        ("slot", slot, i64),
        ("size", consumed_size, i64)
    );

    inc_new_counter_info!(
        "serialize-status-cache-ms",
        status_cache_serialize.as_ms() as usize
    );
    Ok(())
}

pub fn remove_snapshot<P: AsRef<Path>>(slot: Slot, snapshot_path: P) -> Result<()> {
    let slot_snapshot_dir = get_bank_snapshot_dir(&snapshot_path, slot);
    // Remove the snapshot directory for this slot
    fs::remove_dir_all(slot_snapshot_dir)?;
    Ok(())
}

/// Rebuild a bank from a snapshot archive
#[allow(clippy::too_many_arguments)]
pub fn bank_from_snapshot_archive<P: AsRef<Path>>(
    account_paths: &[PathBuf],
    frozen_account_pubkeys: &[Pubkey],
    snapshot_path: &Path,
    snapshot_tar: P,
    archive_format: ArchiveFormat,
    genesis_config: &GenesisConfig,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&Builtins>,
    account_indexes: AccountSecondaryIndexes,
    accounts_db_caching_enabled: bool,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
) -> Result<Bank> {
    let unpack_dir = tempfile::Builder::new()
        .prefix(TMP_SNAPSHOT_PREFIX)
        .tempdir_in(snapshot_path)?;

    let unpacked_append_vec_map = untar_snapshot_in(
        &snapshot_tar,
        &unpack_dir.as_ref(),
        account_paths,
        archive_format,
    )?;

    let mut measure = Measure::start("bank rebuild from snapshot");
    let unpacked_snapshots_dir = unpack_dir.as_ref().join("snapshots");
    let unpacked_version_file = unpack_dir.as_ref().join("version");

    let mut snapshot_version = String::new();
    File::open(unpacked_version_file).and_then(|mut f| f.read_to_string(&mut snapshot_version))?;

    let bank = rebuild_bank_from_snapshots(
        snapshot_version.trim(),
        frozen_account_pubkeys,
        &unpacked_snapshots_dir,
        account_paths,
        unpacked_append_vec_map,
        genesis_config,
        debug_keys,
        additional_builtins,
        account_indexes,
        accounts_db_caching_enabled,
        limit_load_slot_count_from_snapshot,
        shrink_ratio,
    )?;

    if !bank.verify_snapshot_bank() {
        panic!("Snapshot bank for slot {} failed to verify", bank.slot());
    }
    measure.stop();
    info!("{}", measure);

    Ok(bank)
}

/// Rebuild a bank from an incremental snapshot archive and its corresponding full snapshot archive
#[allow(clippy::too_many_arguments)]
pub fn bank_from_incremental_snapshot_archive<P: AsRef<Path>>(
    account_paths: &[PathBuf],
    frozen_account_pubkeys: &[Pubkey],
    snapshot_path: &Path,
    full_snapshot_archive_path: P,
    incremental_snapshot_archive_path: P,
    archive_format: ArchiveFormat,
    genesis_config: &GenesisConfig,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&Builtins>,
    account_indexes: AccountSecondaryIndexes,
    accounts_db_caching_enabled: bool,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
) -> Result<Bank> {
    let listfiles = |dir| {
        debug!("bprumo DEBUG: files in  {:?}:", dir);
        for entry in walkdir::WalkDir::new(dir).follow_links(true) {
            debug!("bprumo DEBUG:\t\t{:?}", entry.unwrap().path());
        }
    };

    debug!(
        "bprumo DEBUG: bank_from_incremental_snapshot_archive(), fss path: {:?}, iss path: {:?}",
        full_snapshot_archive_path.as_ref(),
        incremental_snapshot_archive_path.as_ref()
    );

    check_are_snapshots_compatible(
        &full_snapshot_archive_path,
        &incremental_snapshot_archive_path,
    )?;

    ////////// Full Snapshot

    let fss_unpack_dir = tempfile::Builder::new()
        .prefix(TMP_SNAPSHOT_PREFIX)
        .tempdir_in(snapshot_path)?;

    let fss_unpacked_snapshots_dir = fss_unpack_dir.as_ref().join("snapshots");

    let fss_unpacked_append_vec_map = untar_snapshot_in(
        &full_snapshot_archive_path,
        fss_unpack_dir.as_ref(),
        account_paths,
        archive_format,
    )?;

    debug!("bprumo DEBUG: after FSS untar");
    listfiles(fss_unpack_dir.as_ref());

    debug!("bprumo DEBUG: list files in account paths");
    for p in account_paths.iter() {
        listfiles(p.as_ref());
    }

    ////////// Incremental Snapshot

    let iss_unpack_dir = tempfile::Builder::new()
        .prefix(TMP_INCREMENTAL_SNAPSHOT_PREFIX)
        .tempdir_in(snapshot_path)?;

    let iss_unpacked_snapshots_dir = iss_unpack_dir.as_ref().join("snapshots");

    let iss_unpacked_append_vec_map = untar_snapshot_in(
        &incremental_snapshot_archive_path,
        iss_unpack_dir.as_ref(),
        account_paths,
        archive_format,
    )?;

    debug!("bprumo DEBUG: after ISS untar");
    listfiles(iss_unpack_dir.as_ref());

    debug!("bprumo DEBUG: list files in account paths");
    for p in account_paths.iter() {
        listfiles(p.as_ref());
    }

    ///////// do rebuild

    let mut unpacked_append_vec_map = fss_unpacked_append_vec_map;
    unpacked_append_vec_map.extend(iss_unpacked_append_vec_map.into_iter());

    let unpacked_version_file = iss_unpack_dir.as_ref().join("version");
    let mut snapshot_version = String::new();
    File::open(unpacked_version_file).and_then(|mut f| f.read_to_string(&mut snapshot_version))?;

    let mut measure = Measure::start("bank rebuild from snapshots");

    let bank = rebuild_bank_from_incremental_snapshots(
        snapshot_version.trim(),
        frozen_account_pubkeys,
        &fss_unpacked_snapshots_dir,
        &iss_unpacked_snapshots_dir,
        account_paths,
        unpacked_append_vec_map,
        genesis_config,
        debug_keys,
        additional_builtins,
        account_indexes,
        accounts_db_caching_enabled,
        limit_load_slot_count_from_snapshot,
        shrink_ratio,
    )?;

    if !bank.verify_snapshot_bank() {
        panic!("Snapshot bank for slot {} failed to verify", bank.slot());
    }
    measure.stop();
    info!("{}", measure);

    Ok(bank)
}

/// Check if an incremental snapshot is compatible with a full snapshot.  This function parses the
/// paths to see if the incremental snapshot's base slot is the same as the full snapshot's slot.
/// Return an error if they are incompatible (or if the paths cannot be parsed).
fn check_are_snapshots_compatible<P, Q>(
    full_snapshot_archive_path: P,
    incremental_snapshot_archive_path: Q,
) -> Result<()>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    fn path_to_filename_str(path: &Path) -> Result<&str> {
        path.file_name()
            .ok_or(SnapshotError::PathParseError("Could not get file name!"))?
            .to_str()
            .ok_or(SnapshotError::PathParseError("Could not get &str!"))
    }

    let fss_filename = path_to_filename_str(full_snapshot_archive_path.as_ref())?;
    let (fss_slot, _, _) = parse_snapshot_archive_filename(fss_filename).ok_or(
        SnapshotError::PathParseError("Could not parse snapshot archive's filename!"),
    )?;

    let iss_filename = path_to_filename_str(incremental_snapshot_archive_path.as_ref())?;
    let (iss_base_slot, _, _, _) = parse_incremental_snapshot_archive_filename(iss_filename)
        .ok_or({
            SnapshotError::PathParseError(
                "Could not parse incremental snapshot archive's filename!",
            )
        })?;

    (fss_slot == iss_base_slot)
        .then(|| ())
        .ok_or(SnapshotError::IncompatibleSnapshots(
            fss_slot,
            iss_base_slot,
        ))
}

pub fn get_snapshot_archive_path(
    snapshot_output_dir: PathBuf,
    snapshot_hash: &(Slot, Hash),
    archive_format: ArchiveFormat,
) -> PathBuf {
    snapshot_output_dir.join(format!(
        "snapshot-{}-{}.{}",
        snapshot_hash.0,
        snapshot_hash.1,
        get_archive_ext(archive_format),
    ))
}

fn get_incremental_snapshot_archive_path(
    snapshot_output_dir: PathBuf,
    full_snapshot_slot: Slot,
    bank_slot_and_hash: &(Slot, Hash),
    archive_format: ArchiveFormat,
) -> PathBuf {
    snapshot_output_dir.join(format!(
        "incremental-snapshot-{}-{}-{}.{}",
        full_snapshot_slot,
        bank_slot_and_hash.0,
        bank_slot_and_hash.1,
        get_archive_ext(archive_format),
    ))
}

fn archive_format_from_str(archive_format: &str) -> Option<ArchiveFormat> {
    match archive_format {
        "tar.bz2" => Some(ArchiveFormat::TarBzip2),
        "tar.gz" => Some(ArchiveFormat::TarGzip),
        "tar.zst" => Some(ArchiveFormat::TarZstd),
        "tar" => Some(ArchiveFormat::Tar),
        _ => None,
    }
}

/// Parse a snapshot archive filename into its Slot, Hash, and Archive Format
fn parse_snapshot_archive_filename(archive_filename: &str) -> Option<(Slot, Hash, ArchiveFormat)> {
    let regex = Regex::new(SNAPSHOT_ARCHIVE_FILENAME_REGEX);

    regex.ok()?.captures(archive_filename).and_then(|captures| {
        let slot = captures.get(1).map(|x| x.as_str().parse::<Slot>())?.ok()?;
        let hash = captures.get(2).map(|x| x.as_str().parse::<Hash>())?.ok()?;
        let archive_format = captures
            .get(3)
            .map(|x| archive_format_from_str(x.as_str()))??;

        Some((slot, hash, archive_format))
    })
}

/// Parse an incremental snapshot archive filename into its base Slot, actual Slot, Hash, and Archive Format
fn parse_incremental_snapshot_archive_filename(
    archive_filename: &str,
) -> Option<(Slot, Slot, Hash, ArchiveFormat)> {
    let regex = Regex::new(INCREMENTAL_SNAPSHOT_ARCHIVE_FILENAME_REGEX);

    regex.ok()?.captures(archive_filename).and_then(|captures| {
        let base_slot = captures.get(1).map(|x| x.as_str().parse::<Slot>())?.ok()?;
        let slot = captures.get(2).map(|x| x.as_str().parse::<Slot>())?.ok()?;
        let hash = captures.get(3).map(|x| x.as_str().parse::<Hash>())?.ok()?;
        let archive_format = captures
            .get(4)
            .map(|x| archive_format_from_str(x.as_str()))??;

        Some((base_slot, slot, hash, archive_format))
    })
}

/// Get a list of the snapshot archives in a directory
fn get_snapshot_archives<P: AsRef<Path>>(snapshot_output_dir: P) -> Vec<SnapshotArchiveInfo> {
    match fs::read_dir(&snapshot_output_dir) {
        Err(err) => {
            info!("Unable to read snapshot directory: {}", err);
            vec![]
        }
        Ok(files) => files
            .filter_map(|entry| {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(parse_results) = parse_snapshot_archive_filename(
                            path.file_name().unwrap().to_str().unwrap(),
                        ) {
                            return Some((path, parse_results));
                        }
                    }
                }
                None
            })
            .collect(),
    }
}

/// Get a sorted list of the snapshot archives in a directory
fn get_sorted_snapshot_archives<P: AsRef<Path>>(
    snapshot_output_dir: P,
) -> Vec<SnapshotArchiveInfo> {
    let mut snapshot_archives = get_snapshot_archives(snapshot_output_dir);
    sort_snapshot_archives(&mut snapshot_archives);
    snapshot_archives
}

/// Sort the list of snapshot archives by slot, in descending order
fn sort_snapshot_archives(snapshot_archives: &mut Vec<SnapshotArchiveInfo>) {
    snapshot_archives.sort_unstable_by(|a, b| (b.1).0.cmp(&(a.1).0));
}

// bprumo TODO: can combine this function with get_snapshot_archives by passing in the function to
// parse the filename
/// Get a list of the incremental snapshot archives in a directory
fn get_incremental_snapshot_archives<P: AsRef<Path>>(
    snapshot_output_dir: P,
) -> Vec<IncrementalSnapshotArchiveInfo> {
    match fs::read_dir(&snapshot_output_dir) {
        Err(err) => {
            info!("Unable to read snapshot directory: {}", err);
            vec![]
        }
        Ok(files) => files
            .filter_map(|entry| {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(parse_results) = parse_incremental_snapshot_archive_filename(
                            path.file_name().unwrap().to_str().unwrap(),
                        ) {
                            return Some((path, parse_results));
                        }
                    }
                }
                None
            })
            .collect(),
    }
}

/// Get a sorted list of the incremental snapshot archives in a directory
#[cfg(test)]
fn get_sorted_incremental_snapshot_archives<P: AsRef<Path>>(
    snapshot_output_dir: P,
) -> Vec<IncrementalSnapshotArchiveInfo> {
    let mut incremental_snapshot_archives = get_incremental_snapshot_archives(snapshot_output_dir);
    sort_incremental_snapshot_archives(&mut incremental_snapshot_archives);
    incremental_snapshot_archives
}

/// Sort the list of incremental snapshot archives, first by full snapshot slot in descending
/// order, then by incremental snapshot slot in descending order
fn sort_incremental_snapshot_archives(
    incremental_snapshot_archives: &mut Vec<IncrementalSnapshotArchiveInfo>,
) {
    incremental_snapshot_archives
        .sort_unstable_by(|a, b| (b.1).0.cmp(&(a.1).0).then((b.1).1.cmp(&(a.1).1)));
}

/// Get the highest slot of the snapshots in a directory
pub fn get_highest_snapshot_archive_slot<P: AsRef<Path>>(snapshot_output_dir: P) -> Option<Slot> {
    get_highest_snapshot_archive_path(snapshot_output_dir).map(|(_, (slot, _, _))| slot)
}

/// Get the highest slot of the incremental snapshots in a directory, for a given full snapshot
/// slot
pub fn get_highest_incremental_snapshot_archive_slot<P: AsRef<Path>>(
    snapshot_output_dir: P,
    full_snapshot_slot: Slot,
) -> Option<Slot> {
    get_highest_incremental_snapshot_archive_path(snapshot_output_dir, full_snapshot_slot)
        .map(|(_, (_, slot, _, _))| slot)
}

/// Get the path (and metadata) for the snapshot archive with the highest slot in a directory
pub fn get_highest_snapshot_archive_path<P: AsRef<Path>>(
    snapshot_output_dir: P,
) -> Option<SnapshotArchiveInfo> {
    get_sorted_snapshot_archives(snapshot_output_dir)
        .into_iter()
        .next()
}

/// Get the path for the incremental snapshot archive with the highest slot, for a given full
/// snapshot slot, in a directory
pub fn get_highest_incremental_snapshot_archive_path<P: AsRef<Path>>(
    snapshot_output_dir: P,
    full_snapshot_slot: Slot,
) -> Option<IncrementalSnapshotArchiveInfo> {
    // Do not call get_sorted_incremental_snapshot_archives() here!  Since we want to filter down
    // to only the incremental snapshot archives that have the same full snapshot slot as the value
    // passed in, perform the filtering before sorting to avoid doing unnecessary work.
    let mut incremental_snapshot_archives = get_incremental_snapshot_archives(snapshot_output_dir)
        .into_iter()
        .filter(|(_, (possible_full_snapshot_slot, _, _, _))| {
            *possible_full_snapshot_slot == full_snapshot_slot
        })
        .collect::<Vec<_>>();
    sort_incremental_snapshot_archives(&mut incremental_snapshot_archives);
    incremental_snapshot_archives.into_iter().next()
}

pub fn purge_old_snapshot_archives<P: AsRef<Path>>(
    snapshot_output_dir: P,
    maximum_snapshots_to_retain: usize,
) {
    info!(
        "Purging old snapshots in {:?}, retaining {}",
        snapshot_output_dir.as_ref(),
        maximum_snapshots_to_retain
    );
    let mut archives = get_sorted_snapshot_archives(snapshot_output_dir);
    // Keep the oldest snapshot so we can always play the ledger from it.
    archives.pop();
    let max_snaps = max(1, maximum_snapshots_to_retain);
    for old_archive in archives.into_iter().skip(max_snaps) {
        fs::remove_file(old_archive.0)
            .unwrap_or_else(|err| info!("Failed to remove old snapshot: {:}", err));
    }
}

fn untar_snapshot_in<P: AsRef<Path>>(
    snapshot_tar: P,
    unpack_dir: &Path,
    account_paths: &[PathBuf],
    archive_format: ArchiveFormat,
) -> Result<UnpackedAppendVecMap> {
    let mut measure = Measure::start("snapshot untar");
    let tar_name = File::open(&snapshot_tar)?;
    let account_paths_map = match archive_format {
        ArchiveFormat::TarBzip2 => {
            let tar = BzDecoder::new(BufReader::new(tar_name));
            let mut archive = Archive::new(tar);
            unpack_snapshot(&mut archive, unpack_dir, account_paths)?
        }
        ArchiveFormat::TarGzip => {
            let tar = GzDecoder::new(BufReader::new(tar_name));
            let mut archive = Archive::new(tar);
            unpack_snapshot(&mut archive, unpack_dir, account_paths)?
        }
        ArchiveFormat::TarZstd => {
            let tar = zstd::stream::read::Decoder::new(BufReader::new(tar_name))?;
            let mut archive = Archive::new(tar);
            unpack_snapshot(&mut archive, unpack_dir, account_paths)?
        }
        ArchiveFormat::Tar => {
            let tar = BufReader::new(tar_name);
            let mut archive = Archive::new(tar);
            unpack_snapshot(&mut archive, unpack_dir, account_paths)?
        }
    };
    measure.stop();
    info!("{}", measure);
    Ok(account_paths_map)
}

#[allow(clippy::too_many_arguments)]
fn rebuild_bank_from_incremental_snapshots(
    snapshot_version: &str,
    frozen_account_pubkeys: &[Pubkey],
    fss_unpacked_snapshots_dir: &Path,
    iss_unpacked_snapshots_dir: &Path,
    account_paths: &[PathBuf],
    unpacked_append_vec_map: UnpackedAppendVecMap,
    genesis_config: &GenesisConfig,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&Builtins>,
    account_indexes: AccountSecondaryIndexes,
    accounts_db_caching_enabled: bool,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
) -> Result<Bank> {
    info!("snapshot version: {}", snapshot_version);

    let snapshot_version_enum =
        SnapshotVersion::maybe_from_string(snapshot_version).ok_or_else(|| {
            get_io_error(&format!(
                "unsupported snapshot version: {}",
                snapshot_version
            ))
        })?;
    let mut fss_snapshot_paths = get_snapshot_paths(&fss_unpacked_snapshots_dir);
    let mut iss_snapshot_paths = get_snapshot_paths(&iss_unpacked_snapshots_dir);
    debug!(
        "bprumo DEBUG rebuild_bank_from_snapshots()\n\tfss snapshot_paths: {:?}\n\tiss snapshot paths: {:?}",
        fss_snapshot_paths,
        iss_snapshot_paths
    );
    if fss_snapshot_paths.len() > 1 || iss_snapshot_paths.len() > 1 {
        return Err(get_io_error("invalid snapshot format"));
    }

    let fss_root_paths = fss_snapshot_paths
        .pop()
        .ok_or_else(|| get_io_error("No snapshots found in snapshots directory"))?;

    let iss_root_paths = iss_snapshot_paths
        .pop()
        .ok_or_else(|| get_io_error("No snapshots found in snapshots directory"))?;

    info!(
        "Loading bank from {} and {}",
        &fss_root_paths.snapshot_file_path.display(),
        &iss_root_paths.snapshot_file_path.display()
    );

    debug!(
        "bprumo DEBUG: rebuild_bank_from_incremental_snapshots(), Loading bank from {} and {}",
        &fss_root_paths.snapshot_file_path.display(),
        &iss_root_paths.snapshot_file_path.display()
    );

    let bank = deserialize_incremental_snapshot_data_file(
        &fss_root_paths.snapshot_file_path,
        &iss_root_paths.snapshot_file_path,
        |mut fss_stream, mut iss_stream| {
            Ok(match snapshot_version_enum {
                SnapshotVersion::V1_2_0 => bank_from_stream_incremental(
                    SerdeStyle::Newer,
                    &mut fss_stream,
                    &mut iss_stream,
                    account_paths,
                    unpacked_append_vec_map,
                    genesis_config,
                    frozen_account_pubkeys,
                    debug_keys,
                    additional_builtins,
                    account_indexes,
                    accounts_db_caching_enabled,
                    limit_load_slot_count_from_snapshot,
                    shrink_ratio,
                ),
            }?)
        },
    )?;

    let status_cache_path = iss_unpacked_snapshots_dir.join(SNAPSHOT_STATUS_CACHE_FILE_NAME);
    let slot_deltas = deserialize_snapshot_data_file(&status_cache_path, |stream| {
        info!(
            "Rebuilding status cache from {}",
            status_cache_path.display()
        );
        let slot_deltas: Vec<BankSlotDelta> = bincode::options()
            .with_limit(MAX_SNAPSHOT_DATA_FILE_SIZE)
            .with_fixint_encoding()
            .allow_trailing_bytes()
            .deserialize_from(stream)?;
        Ok(slot_deltas)
    })?;

    bank.src.append(&slot_deltas);

    info!("Loaded bank for slot: {}", bank.slot());
    Ok(bank)
}

#[allow(clippy::too_many_arguments)]
fn rebuild_bank_from_snapshots(
    snapshot_version: &str,
    frozen_account_pubkeys: &[Pubkey],
    unpacked_snapshots_dir: &Path,
    account_paths: &[PathBuf],
    unpacked_append_vec_map: UnpackedAppendVecMap,
    genesis_config: &GenesisConfig,
    debug_keys: Option<Arc<HashSet<Pubkey>>>,
    additional_builtins: Option<&Builtins>,
    account_indexes: AccountSecondaryIndexes,
    accounts_db_caching_enabled: bool,
    limit_load_slot_count_from_snapshot: Option<usize>,
    shrink_ratio: AccountShrinkThreshold,
) -> Result<Bank> {
    info!("snapshot version: {}", snapshot_version);

    let snapshot_version_enum =
        SnapshotVersion::maybe_from_string(snapshot_version).ok_or_else(|| {
            get_io_error(&format!(
                "unsupported snapshot version: {}",
                snapshot_version
            ))
        })?;
    let mut snapshot_paths = get_snapshot_paths(&unpacked_snapshots_dir);
    debug!(
        "bprumo DEBUG rebuild_bank_from_snapshots()\n\tsnapshot_paths: {:?}",
        snapshot_paths
    );
    if snapshot_paths.len() > 1 {
        return Err(get_io_error("invalid snapshot format"));
    }
    let root_paths = snapshot_paths
        .pop()
        .ok_or_else(|| get_io_error("No snapshots found in snapshots directory"))?;

    info!(
        "Loading bank from {}",
        &root_paths.snapshot_file_path.display()
    );

    debug!(
        "bprumo DEBUG: rebuild_bank_from_snapshots(), Loading bank from {}",
        &root_paths.snapshot_file_path.display()
    );
    let bank = deserialize_snapshot_data_file(&root_paths.snapshot_file_path, |mut stream| {
        Ok(match snapshot_version_enum {
            SnapshotVersion::V1_2_0 => bank_from_stream(
                SerdeStyle::Newer,
                &mut stream,
                account_paths,
                unpacked_append_vec_map,
                genesis_config,
                frozen_account_pubkeys,
                debug_keys,
                additional_builtins,
                account_indexes,
                accounts_db_caching_enabled,
                limit_load_slot_count_from_snapshot,
                shrink_ratio,
            ),
        }?)
    })?;

    let status_cache_path = unpacked_snapshots_dir.join(SNAPSHOT_STATUS_CACHE_FILE_NAME);
    let slot_deltas = deserialize_snapshot_data_file(&status_cache_path, |stream| {
        info!(
            "Rebuilding status cache from {}",
            status_cache_path.display()
        );
        let slot_deltas: Vec<BankSlotDelta> = bincode::options()
            .with_limit(MAX_SNAPSHOT_DATA_FILE_SIZE)
            .with_fixint_encoding()
            .allow_trailing_bytes()
            .deserialize_from(stream)?;
        Ok(slot_deltas)
    })?;

    bank.src.append(&slot_deltas);

    info!("Loaded bank for slot: {}", bank.slot());
    Ok(bank)
}

fn get_snapshot_file_name(slot: Slot) -> String {
    slot.to_string()
}

fn get_bank_snapshot_dir<P: AsRef<Path>>(path: P, slot: Slot) -> PathBuf {
    path.as_ref().join(slot.to_string())
}

fn get_io_error(error: &str) -> SnapshotError {
    warn!("Snapshot Error: {:?}", error);
    SnapshotError::Io(IoError::new(ErrorKind::Other, error))
}

pub fn verify_snapshot_archive<P, Q, R>(
    snapshot_archive: P,
    snapshots_to_verify: Q,
    storages_to_verify: R,
    archive_format: ArchiveFormat,
) where
    P: AsRef<Path>,
    Q: AsRef<Path>,
    R: AsRef<Path>,
{
    let temp_dir = tempfile::TempDir::new().unwrap();
    let unpack_dir = temp_dir.path();
    untar_snapshot_in(
        snapshot_archive,
        &unpack_dir,
        &[unpack_dir.to_path_buf()],
        archive_format,
    )
    .unwrap();

    // Check snapshots are the same
    let unpacked_snapshots = unpack_dir.join("snapshots");
    assert!(!dir_diff::is_different(&snapshots_to_verify, unpacked_snapshots).unwrap());

    // Check the account entries are the same
    let unpacked_accounts = unpack_dir.join("accounts");
    assert!(!dir_diff::is_different(&storages_to_verify, unpacked_accounts).unwrap());
}

pub fn purge_old_snapshots(snapshot_path: &Path) {
    // Remove outdated snapshots
    let slot_snapshot_paths = get_snapshot_paths(snapshot_path);
    let num_to_remove = slot_snapshot_paths.len().saturating_sub(MAX_SNAPSHOTS);
    for slot_files in &slot_snapshot_paths[..num_to_remove] {
        let r = remove_snapshot(slot_files.slot, snapshot_path);
        if r.is_err() {
            warn!("Couldn't remove snapshot at: {:?}", snapshot_path);
        }
    }
}

// Gather the necessary elements for a snapshot of the given `root_bank`
pub fn snapshot_bank(
    root_bank: &Bank,
    status_cache_slot_deltas: Vec<BankSlotDelta>,
    accounts_package_sender: &AccountsPackageSender,
    snapshot_path: &Path,
    snapshot_package_output_path: &Path,
    snapshot_version: SnapshotVersion,
    archive_format: &ArchiveFormat,
    hash_for_testing: Option<Hash>,
) -> Result<()> {
    let storages: Vec<_> = root_bank.get_snapshot_storages();
    let mut add_snapshot_time = Measure::start("add-snapshot-ms");
    add_snapshot(snapshot_path, &root_bank, &storages, snapshot_version)?;
    add_snapshot_time.stop();
    inc_new_counter_info!("add-snapshot-ms", add_snapshot_time.as_ms() as usize);

    // Package the relevant snapshots
    let slot_snapshot_paths = get_snapshot_paths(snapshot_path);
    let latest_slot_snapshot_paths = slot_snapshot_paths
        .last()
        .expect("no snapshots found in config snapshot_path");

    let package = package_snapshot(
        &root_bank,
        latest_slot_snapshot_paths,
        snapshot_path,
        status_cache_slot_deltas,
        snapshot_package_output_path,
        storages,
        *archive_format,
        snapshot_version,
        hash_for_testing,
    )?;

    accounts_package_sender.send(package)?;

    Ok(())
}

/// Convenience function to create a snapshot archive out of any Bank, regardless of state.  The
/// Bank will be frozen during the process.
///
/// Requires:
///     - `bank` is complete
pub fn bank_to_snapshot_archive<P: AsRef<Path>, Q: AsRef<Path>>(
    snapshot_path: P,
    bank: &Bank,
    snapshot_version: Option<SnapshotVersion>,
    snapshot_package_output_path: Q,
    archive_format: ArchiveFormat,
    thread_pool: Option<&ThreadPool>,
    maximum_snapshots_to_retain: usize,
) -> Result<PathBuf> {
    assert!(bank.is_complete());
    bank.squash(); // Bank may not be a root
    bank.force_flush_accounts_cache();
    bank.clean_accounts(true, false);
    bank.update_accounts_hash();
    bank.rehash(); // Bank accounts may have been manually modified by the caller

    let temp_dir = tempfile::tempdir_in(snapshot_path)?;
    let snapshot_version = snapshot_version.unwrap_or_default();

    let storages: Vec<_> = bank.get_snapshot_storages();
    let slot_snapshot_paths = add_snapshot(&temp_dir, &bank, &storages, snapshot_version)?;
    let package = package_snapshot(
        &bank,
        &slot_snapshot_paths,
        &temp_dir,
        bank.src.slot_deltas(&bank.src.roots()),
        snapshot_package_output_path,
        storages,
        archive_format,
        snapshot_version,
        None,
    )?;

    let package = process_accounts_package_pre(package, thread_pool);

    archive_snapshot_package(&package, maximum_snapshots_to_retain)?;
    Ok(package.tar_output_file)
}

/// Convenience function to create an incremental snapshot archive out of any Bank, regardless of
/// state.  The Bank will be frozen during the process.
///
/// Requires:
///     - `bank` is complete
///     - `bank`'s slot is greater than `full_snapshot_slot`
pub fn bank_to_incremental_snapshot_archive<P: AsRef<Path>, Q: AsRef<Path>>(
    snapshot_path: P,
    bank: &Bank,
    full_snapshot_slot: Slot,
    snapshot_version: Option<SnapshotVersion>,
    snapshot_package_output_path: Q,
    archive_format: ArchiveFormat,
    thread_pool: Option<&ThreadPool>,
    maximum_snapshots_to_retain: usize,
) -> Result<PathBuf>
where
    P: fmt::Debug,
{
    assert!(bank.is_complete());
    assert!(bank.slot() > full_snapshot_slot);

    bank.squash(); // Bank may not be a root
    bank.force_flush_accounts_cache();

    // bprumo TODO: Cleaning still needs to be figured out.  My understanding is that we cannot
    // clean past the last full snapshot slot here, but more testing needs to be done.  For now,
    // I'm not cleaning at all when creating an incremental snapshot, since a full snapshot must
    // have been created first, which would have performed a clean then.  Below are the different
    // `clean_accounts()` options:
    //      bank.clean_accounts(true, false);
    //      bank.clean_accounts_up_to_slot(Some(full_snapshot_slot), false);
    //      bank.clean_accounts_up_to_slot(Some(full_snapshot_slot.saturating_sub(1)), false);

    bank.update_accounts_hash();
    bank.rehash(); // Bank accounts may have been manually modified by the caller

    let storages: SnapshotStorages = bank
        .get_snapshot_storages()
        .into_iter()
        .filter(|storage| storage.first().unwrap().slot() > full_snapshot_slot)
        .collect();

    let temp_dir = tempfile::tempdir_in(snapshot_path)?;
    let snapshot_version = snapshot_version.unwrap_or_default();
    let snapshot_path = add_snapshot(&temp_dir, &bank, &storages, snapshot_version)?;

    let package = package_incremental_snapshot(
        &bank,
        full_snapshot_slot,
        &snapshot_path,
        &temp_dir,
        bank.src.slot_deltas(&bank.src.roots()),
        &snapshot_package_output_path,
        storages,
        archive_format,
        snapshot_version,
        None,
    )?;

    let package =
        process_accounts_package_pre_incremental(package, thread_pool, full_snapshot_slot);

    // bprumo TODO: need to make an archive_incremental_snapshot_package so that it handles the
    // number of full snapshots and incremental snapshots to keep correctly
    archive_snapshot_package(&package, maximum_snapshots_to_retain)?;
    Ok(package.tar_output_file)
}

pub fn process_accounts_package_pre(
    accounts_package: AccountsPackagePre,
    thread_pool: Option<&ThreadPool>,
) -> AccountsPackage {
    let mut time = Measure::start("hash");

    let hash = accounts_package.hash; // temporarily remaining here
    if let Some(expected_hash) = accounts_package.hash_for_testing {
        let sorted_storages = SortedStorages::new(&accounts_package.storages);
        let (hash, lamports) = AccountsDb::calculate_accounts_hash_without_index(
            &sorted_storages,
            thread_pool,
            crate::accounts_hash::HashStats::default(),
            false,
        )
        .unwrap();

        assert_eq!(accounts_package.expected_capitalization, lamports);

        assert_eq!(expected_hash, hash);
    };
    time.stop();

    datapoint_info!(
        "accounts_hash_verifier",
        ("calculate_hash", time.as_us(), i64),
    );

    let tar_output_file = get_snapshot_archive_path(
        accounts_package.snapshot_output_dir,
        &(accounts_package.slot, hash),
        accounts_package.archive_format,
    );

    AccountsPackage::new(
        accounts_package.slot,
        accounts_package.block_height,
        accounts_package.slot_deltas,
        accounts_package.snapshot_links,
        accounts_package.storages,
        tar_output_file,
        hash,
        accounts_package.archive_format,
        accounts_package.snapshot_version,
    )
}

fn process_accounts_package_pre_incremental(
    accounts_package: AccountsPackagePre,
    thread_pool: Option<&ThreadPool>,
    full_snapshot_slot: Slot,
) -> AccountsPackage {
    let mut time = Measure::start("hash");

    let hash = accounts_package.hash; // temporarily remaining here
    if let Some(expected_hash) = accounts_package.hash_for_testing {
        let sorted_storages = SortedStorages::new(&accounts_package.storages);
        let (hash, lamports) = AccountsDb::calculate_accounts_hash_without_index(
            &sorted_storages,
            thread_pool,
            crate::accounts_hash::HashStats::default(),
            false,
        )
        .unwrap();

        assert_eq!(accounts_package.expected_capitalization, lamports);

        assert_eq!(expected_hash, hash);
    };
    time.stop();

    datapoint_info!(
        "accounts_hash_verifier",
        ("calculate_hash", time.as_us(), i64),
    );

    let tar_output_file = get_incremental_snapshot_archive_path(
        accounts_package.snapshot_output_dir,
        full_snapshot_slot,
        &(accounts_package.slot, hash),
        accounts_package.archive_format,
    );

    AccountsPackage::new(
        accounts_package.slot,
        accounts_package.block_height,
        accounts_package.slot_deltas,
        accounts_package.snapshot_links,
        accounts_package.storages,
        tar_output_file,
        hash,
        accounts_package.archive_format,
        accounts_package.snapshot_version,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use bincode::{deserialize_from, serialize_into};
    use solana_sdk::{
        genesis_config::create_genesis_config,
        signature::{Keypair, Signer},
    };
    use std::mem::size_of;

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

        let actual_data = deserialize_snapshot_data_file_capped(
            &temp_dir.path().join("data-file"),
            expected_consumed_size,
            |stream| Ok(deserialize_from::<_, u32>(stream)?),
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

        let result = deserialize_snapshot_data_file_capped(
            &temp_dir.path().join("data-file"),
            expected_consumed_size - 1,
            |stream| Ok(deserialize_from::<_, u32>(stream)?),
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

        let result = deserialize_snapshot_data_file_capped(
            &temp_dir.path().join("data-file"),
            expected_consumed_size * 2,
            |stream| Ok(deserialize_from::<_, u32>(stream)?),
        );
        assert_matches!(result, Err(SnapshotError::Io(ref message)) if message.to_string().starts_with("invalid snapshot data file"));
    }

    #[test]
    fn test_parse_snapshot_archive_filename() {
        solana_logger::setup();
        assert_eq!(
            parse_snapshot_archive_filename(&format!("snapshot-42-{}.tar.bz2", Hash::default())),
            Some((42, Hash::default(), ArchiveFormat::TarBzip2))
        );
        assert_eq!(
            parse_snapshot_archive_filename(&format!("snapshot-43-{}.tar.zst", Hash::default())),
            Some((43, Hash::default(), ArchiveFormat::TarZstd))
        );
        assert_eq!(
            parse_snapshot_archive_filename(&format!("snapshot-44-{}.tar", Hash::default())),
            Some((44, Hash::default(), ArchiveFormat::Tar))
        );

        assert!(parse_snapshot_archive_filename("invalid").is_none());
        assert!(parse_snapshot_archive_filename("snapshot-bad!slot-bad!hash.bad!ext").is_none());

        assert!(parse_snapshot_archive_filename("snapshot-12345678-bad!hash.bad!ext").is_none());
        assert!(parse_snapshot_archive_filename(&format!(
            "snapshot-12345678-{}.bad!ext",
            Hash::new_unique()
        ))
        .is_none());
        assert!(parse_snapshot_archive_filename("snapshot-12345678-bad!hash.tar").is_none());

        assert!(parse_snapshot_archive_filename(&format!(
            "snapshot-bad!slot-{}.bad!ext",
            Hash::new_unique()
        ))
        .is_none());
        assert!(parse_snapshot_archive_filename(&format!(
            "snapshot-12345678-{}.bad!ext",
            Hash::new_unique()
        ))
        .is_none());
        assert!(parse_snapshot_archive_filename(&format!(
            "snapshot-bad!slot-{}.tar",
            Hash::new_unique()
        ))
        .is_none());

        assert!(parse_snapshot_archive_filename("snapshot-bad!slot-bad!hash.tar").is_none());
        assert!(parse_snapshot_archive_filename("snapshot-12345678-bad!hash.tar").is_none());
        assert!(parse_snapshot_archive_filename(&format!(
            "snapshot-bad!slot-{}.tar",
            Hash::new_unique()
        ))
        .is_none());
    }

    #[test]
    fn test_parse_incremental_snapshot_archive_filename() {
        solana_logger::setup();
        assert_eq!(
            parse_incremental_snapshot_archive_filename(&format!(
                "incremental-snapshot-42-123-{}.tar.bz2",
                Hash::default()
            )),
            Some((42, 123, Hash::default(), ArchiveFormat::TarBzip2))
        );
        assert_eq!(
            parse_incremental_snapshot_archive_filename(&format!(
                "incremental-snapshot-43-234-{}.tar.zst",
                Hash::default()
            )),
            Some((43, 234, Hash::default(), ArchiveFormat::TarZstd))
        );
        assert_eq!(
            parse_incremental_snapshot_archive_filename(&format!(
                "incremental-snapshot-44-345-{}.tar",
                Hash::default()
            )),
            Some((44, 345, Hash::default(), ArchiveFormat::Tar))
        );

        assert!(parse_incremental_snapshot_archive_filename("invalid").is_none());
        assert!(parse_incremental_snapshot_archive_filename(&format!(
            "snapshot-42-{}.tar",
            Hash::new_unique()
        ))
        .is_none());
        assert!(parse_incremental_snapshot_archive_filename(
            "incremental-snapshot-bad!slot-bad!slot-bad!hash.bad!ext"
        )
        .is_none());

        assert!(parse_incremental_snapshot_archive_filename(&format!(
            "incremental-snapshot-bad!slot-56785678-{}.tar",
            Hash::new_unique()
        ))
        .is_none());

        assert!(parse_incremental_snapshot_archive_filename(&format!(
            "incremental-snapshot-12345678-bad!slot-{}.tar",
            Hash::new_unique()
        ))
        .is_none());

        assert!(parse_incremental_snapshot_archive_filename(
            "incremental-snapshot-12341234-56785678-bad!HASH.tar"
        )
        .is_none());

        assert!(parse_incremental_snapshot_archive_filename(&format!(
            "incremental-snapshot-12341234-56785678-{}.bad!ext",
            Hash::new_unique()
        ))
        .is_none());
    }

    #[test]
    fn test_check_are_snapshots_compatible() {
        solana_logger::setup();
        let slot1: Slot = 1234;
        let slot2: Slot = 5678;
        let slot3: Slot = 999_999;

        assert!(check_are_snapshots_compatible(
            &format!("/dir/snapshot-{}-{}.tar", slot1, Hash::new_unique()),
            &format!(
                "/dir/incremental-snapshot-{}-{}-{}.tar",
                slot1,
                slot2,
                Hash::new_unique()
            ),
        )
        .is_ok());

        assert!(check_are_snapshots_compatible(
            &format!("/dir/snapshot-{}-{}.tar", slot1, Hash::new_unique()),
            &format!(
                "/dir/incremental-snapshot-{}-{}-{}.tar",
                slot2,
                slot3,
                Hash::new_unique()
            ),
        )
        .is_err());
    }

    /// A test helper function that creates full and incremental snapshot archive files.  Creates
    /// full snapshot files in the range (`min_fss_slot`, `max_fss_slot`], and incremental snapshot
    /// files in the range (`min_iss_slot`, `max_iss_slot`].  Additionally, "bad" files are created
    /// for both full and incremental snapshots to ensure the tests properly filter them out.
    fn common_create_snapshot_archive_files(
        snapshot_dir: &Path,
        min_fss_slot: Slot,
        max_fss_slot: Slot,
        min_iss_slot: Slot,
        max_iss_slot: Slot,
    ) {
        for fss_slot in min_fss_slot..max_fss_slot {
            for iss_slot in min_iss_slot..max_iss_slot {
                let snapshot_filename = format!(
                    "incremental-snapshot-{}-{}-{}.tar",
                    fss_slot,
                    iss_slot,
                    Hash::default()
                );
                let snapshot_filepath = snapshot_dir.join(snapshot_filename);
                File::create(snapshot_filepath).unwrap();
            }

            let snapshot_filename = format!("snapshot-{}-{}.tar", fss_slot, Hash::default());
            let snapshot_filepath = snapshot_dir.join(snapshot_filename);
            File::create(snapshot_filepath).unwrap();

            // Add in an incremental snapshot with a bad filename and high slot to ensure filename are filtered and sorted correctly
            let bad_filename = format!(
                "incremental-snapshot-{}-{}-bad!hash.tar",
                fss_slot,
                max_iss_slot + 1,
            );
            let bad_filepath = snapshot_dir.join(bad_filename);
            File::create(bad_filepath).unwrap();
        }

        // Add in a snapshot with a bad filename and high slot to ensure filename are filtered and
        // sorted correctly
        let bad_filename = format!("snapshot-{}-bad!hash.tar", max_fss_slot + 1);
        let bad_filepath = snapshot_dir.join(bad_filename);
        File::create(bad_filepath).unwrap();
    }

    #[test]
    fn test_get_snapshot_archives() {
        solana_logger::setup();
        let temp_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let min_slot = 123;
        let max_slot = 456;
        common_create_snapshot_archive_files(
            temp_snapshot_archives_dir.path(),
            min_slot,
            max_slot,
            0,
            0,
        );

        let results = get_snapshot_archives(temp_snapshot_archives_dir);
        assert_eq!(results.len() as Slot, max_slot - min_slot);
    }

    #[test]
    fn test_get_sorted_snapshot_archives() {
        solana_logger::setup();
        let temp_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let min_slot = 12;
        let max_slot = 45;
        common_create_snapshot_archive_files(
            temp_snapshot_archives_dir.path(),
            min_slot,
            max_slot,
            0,
            0,
        );

        let results = get_sorted_snapshot_archives(temp_snapshot_archives_dir);
        assert_eq!(results.len() as Slot, max_slot - min_slot);
        assert_eq!(results[0].1 .0, max_slot - 1);
    }

    #[test]
    fn test_get_incremental_snapshot_archives() {
        solana_logger::setup();
        let temp_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let min_fss_slot = 12;
        let max_fss_slot = 23;
        let min_iss_slot = 34;
        let max_iss_slot = 45;
        common_create_snapshot_archive_files(
            temp_snapshot_archives_dir.path(),
            min_fss_slot,
            max_fss_slot,
            min_iss_slot,
            max_iss_slot,
        );

        let results = get_incremental_snapshot_archives(temp_snapshot_archives_dir);
        assert_eq!(
            results.len() as Slot,
            (max_fss_slot - min_fss_slot) * (max_iss_slot - min_iss_slot)
        );
    }

    #[test]
    fn test_get_sorted_incremental_snapshot_archives() {
        solana_logger::setup();
        let temp_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let min_fss_slot = 12;
        let max_fss_slot = 23;
        let min_iss_slot = 34;
        let max_iss_slot = 45;
        common_create_snapshot_archive_files(
            temp_snapshot_archives_dir.path(),
            min_fss_slot,
            max_fss_slot,
            min_iss_slot,
            max_iss_slot,
        );

        let results = get_sorted_incremental_snapshot_archives(temp_snapshot_archives_dir);
        assert_eq!(
            results.len() as Slot,
            (max_fss_slot - min_fss_slot) * (max_iss_slot - min_iss_slot)
        );
        assert_eq!(results[0].1 .0, max_fss_slot - 1);
        assert_eq!(results[0].1 .1, max_iss_slot - 1);
    }

    #[test]
    fn test_get_highest_snapshot_archive_slot() {
        solana_logger::setup();
        let temp_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let min_slot = 123;
        let max_slot = 456;
        common_create_snapshot_archive_files(
            temp_snapshot_archives_dir.path(),
            min_slot,
            max_slot,
            0,
            0,
        );

        assert_eq!(
            get_highest_snapshot_archive_slot(temp_snapshot_archives_dir.path()),
            Some(max_slot - 1)
        );
    }

    #[test]
    fn test_get_highest_incremental_snapshot_slot() {
        solana_logger::setup();
        let temp_snapshot_archives_dir = tempfile::TempDir::new().unwrap();
        let min_fss_slot = 12;
        let max_fss_slot = 23;
        let min_iss_slot = 34;
        let max_iss_slot = 45;
        common_create_snapshot_archive_files(
            temp_snapshot_archives_dir.path(),
            min_fss_slot,
            max_fss_slot,
            min_iss_slot,
            max_iss_slot,
        );

        for fss_slot in min_fss_slot..max_fss_slot {
            assert_eq!(
                get_highest_incremental_snapshot_archive_slot(
                    temp_snapshot_archives_dir.path(),
                    fss_slot
                ),
                Some(max_iss_slot - 1)
            );
        }

        assert_eq!(
            get_highest_incremental_snapshot_archive_slot(
                temp_snapshot_archives_dir.path(),
                max_fss_slot
            ),
            None
        );
    }

    fn common_test_purge_old_snapshot_archives(
        snapshot_names: &[&String],
        maximum_snapshots_to_retain: usize,
        expected_snapshots: &[&String],
    ) {
        let temp_snap_dir = tempfile::TempDir::new().unwrap();

        for snap_name in snapshot_names {
            let snap_path = temp_snap_dir.path().join(&snap_name);
            let mut _snap_file = File::create(snap_path);
        }
        purge_old_snapshot_archives(temp_snap_dir.path(), maximum_snapshots_to_retain);

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
            assert!(retained_snaps.contains(snap_name.as_str()));
        }
        assert!(retained_snaps.len() == expected_snapshots.len());
    }

    #[test]
    fn test_purge_old_snapshot_archives() {
        // Create 3 snapshots, retaining 1,
        // expecting the oldest 1 and the newest 1 are retained
        let snap1_name = format!("snapshot-1-{}.tar.zst", Hash::default());
        let snap2_name = format!("snapshot-3-{}.tar.zst", Hash::default());
        let snap3_name = format!("snapshot-50-{}.tar.zst", Hash::default());
        let snapshot_names = vec![&snap1_name, &snap2_name, &snap3_name];
        let expected_snapshots = vec![&snap1_name, &snap3_name];
        common_test_purge_old_snapshot_archives(&snapshot_names, 1, &expected_snapshots);

        // retaining 0, the expectation is the same as for 1, as at least 1 newest is expected to be retained
        common_test_purge_old_snapshot_archives(&snapshot_names, 0, &expected_snapshots);

        // retaining 2, all three should be retained
        let expected_snapshots = vec![&snap1_name, &snap2_name, &snap3_name];
        common_test_purge_old_snapshot_archives(&snapshot_names, 2, &expected_snapshots);
    }

    /// Test roundtrip of bank to snapshot, then back again.  This test creates the simplest bank
    /// possible, so the contents of the snapshot archive will be quite minimal.
    #[test]
    fn test_roundtrip_bank_to_snapshot_to_bank_simple() {
        solana_logger::setup();
        let genesis_config = GenesisConfig::default();
        let original_bank = Bank::new(&genesis_config);

        while !original_bank.is_complete() {
            original_bank.register_tick(&Hash::new_unique());
        }

        let accounts_dir = tempfile::TempDir::new().unwrap();
        let snapshot_dir = tempfile::TempDir::new().unwrap();
        let snapshot_package_output_dir = tempfile::TempDir::new().unwrap();
        let snapshot_archive_format = ArchiveFormat::Tar;

        let snapshot_archive_path = bank_to_snapshot_archive(
            snapshot_dir.path(),
            &original_bank,
            None,
            snapshot_package_output_dir.path(),
            snapshot_archive_format,
            None,
            1,
        )
        .unwrap();

        let roundtrip_bank = bank_from_snapshot_archive(
            &[PathBuf::from(accounts_dir.path())],
            &[],
            snapshot_dir.path(),
            &snapshot_archive_path,
            snapshot_archive_format,
            &genesis_config,
            None,
            None,
            AccountSecondaryIndexes::default(),
            false,
            None,
            AccountShrinkThreshold::default(),
        )
        .unwrap();

        assert_eq!(original_bank, roundtrip_bank);
    }

    /// Test roundtrip of bank to snapshot, then back again.  This test is more involved than the
    /// simple version above; creating multiple banks over multiple slots and doing multiple
    /// transfers.  So this snapshot should contain more data.
    #[test]
    fn test_roundtrip_bank_to_snapshot_to_bank_complex() {
        solana_logger::setup();
        let collector = Pubkey::new_unique();
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let key3 = Keypair::new();
        let key4 = Keypair::new();
        let key5 = Keypair::new();

        let (genesis_config, mint_keypair) = create_genesis_config(1_000_000);
        let bank0 = Arc::new(Bank::new(&genesis_config));
        bank0.transfer(1, &mint_keypair, &key1.pubkey()).unwrap();
        bank0.transfer(2, &mint_keypair, &key2.pubkey()).unwrap();
        bank0.transfer(3, &mint_keypair, &key3.pubkey()).unwrap();
        while !bank0.is_complete() {
            bank0.register_tick(&Hash::new_unique());
        }

        let slot = 1;
        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &collector, slot));
        bank1.transfer(3, &mint_keypair, &key3.pubkey()).unwrap();
        bank1.transfer(4, &mint_keypair, &key4.pubkey()).unwrap();
        bank1.transfer(5, &mint_keypair, &key5.pubkey()).unwrap();
        while !bank1.is_complete() {
            bank1.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank2 = Arc::new(Bank::new_from_parent(&bank1, &collector, slot));
        bank2.transfer(1, &mint_keypair, &key1.pubkey()).unwrap();
        while !bank2.is_complete() {
            bank2.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank3 = Arc::new(Bank::new_from_parent(&bank2, &collector, slot));
        bank3.transfer(1, &mint_keypair, &key1.pubkey()).unwrap();
        while !bank3.is_complete() {
            bank3.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank4 = Arc::new(Bank::new_from_parent(&bank3, &collector, slot));
        bank4.transfer(1, &mint_keypair, &key1.pubkey()).unwrap();
        while !bank4.is_complete() {
            bank4.register_tick(&Hash::new_unique());
        }

        let accounts_dir = tempfile::TempDir::new().unwrap();
        let snapshot_dir = tempfile::TempDir::new().unwrap();
        let snapshot_package_output_dir = tempfile::TempDir::new().unwrap();
        let snapshot_archive_format = ArchiveFormat::Tar;

        let full_snapshot_archive_path = bank_to_snapshot_archive(
            snapshot_dir.path(),
            &bank4,
            None,
            snapshot_package_output_dir.path(),
            snapshot_archive_format,
            None,
            std::usize::MAX,
        )
        .unwrap();

        let roundtrip_bank = bank_from_snapshot_archive(
            &[PathBuf::from(accounts_dir.path())],
            &[],
            snapshot_dir.path(),
            &full_snapshot_archive_path,
            snapshot_archive_format,
            &genesis_config,
            None,
            None,
            AccountSecondaryIndexes::default(),
            false,
            None,
            AccountShrinkThreshold::default(),
        )
        .unwrap();

        assert_eq!(*bank4, roundtrip_bank);
    }

    /// Test roundtrip of bank to snapshot, then back again, with an incremental snapshot too.  In
    /// this version, build up a few slots and take a full snapshot.  Continue on a few more slots
    /// and take an incremental snapshot.  Rebuild the bank from both the incremental snapshot and
    /// full snapshot.
    ///
    /// For the full snapshot, touch all the accounts, but only one for the incremental snapshot.
    /// This is intended to mimic the real behavior of transactions, where only a small number of
    /// accounts are modified often, which are captured by the incremental snapshot.  The majority
    /// of the accounts are not modified often, and are captured by the full snapshot.
    #[test]
    fn test_roundtrip_bank_to_incremental_snapshot_to_bank() {
        solana_logger::setup();
        let collector = Pubkey::new_unique();
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let key3 = Keypair::new();
        let key4 = Keypair::new();
        let key5 = Keypair::new();

        let (genesis_config, mint_keypair) = create_genesis_config(1_000_000);
        let bank0 = Arc::new(Bank::new(&genesis_config));
        bank0.transfer(1, &mint_keypair, &key1.pubkey()).unwrap();
        bank0.transfer(2, &mint_keypair, &key2.pubkey()).unwrap();
        bank0.transfer(3, &mint_keypair, &key3.pubkey()).unwrap();
        while !bank0.is_complete() {
            bank0.register_tick(&Hash::new_unique());
        }

        let slot = 1;
        let bank1 = Arc::new(Bank::new_from_parent(&bank0, &collector, slot));
        bank1.transfer(3, &mint_keypair, &key3.pubkey()).unwrap();
        bank1.transfer(4, &mint_keypair, &key4.pubkey()).unwrap();
        bank1.transfer(5, &mint_keypair, &key5.pubkey()).unwrap();
        while !bank1.is_complete() {
            bank1.register_tick(&Hash::new_unique());
        }

        let accounts_dir = tempfile::TempDir::new().unwrap();
        let snapshot_dir = tempfile::TempDir::new().unwrap();
        let snapshot_package_output_dir = tempfile::TempDir::new().unwrap();
        let snapshot_archive_format = ArchiveFormat::Tar;

        let full_snapshot_slot = slot;
        let full_snapshot_archive_path = bank_to_snapshot_archive(
            snapshot_dir.path(),
            &bank1,
            None,
            snapshot_package_output_dir.path(),
            snapshot_archive_format,
            None,
            std::usize::MAX,
        )
        .unwrap();

        let slot = slot + 1;
        let bank2 = Arc::new(Bank::new_from_parent(&bank1, &collector, slot));
        bank2.transfer(1, &mint_keypair, &key1.pubkey()).unwrap();
        while !bank2.is_complete() {
            bank2.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank3 = Arc::new(Bank::new_from_parent(&bank2, &collector, slot));
        bank3.transfer(1, &mint_keypair, &key1.pubkey()).unwrap();
        while !bank3.is_complete() {
            bank3.register_tick(&Hash::new_unique());
        }

        let slot = slot + 1;
        let bank4 = Arc::new(Bank::new_from_parent(&bank3, &collector, slot));
        bank4.transfer(1, &mint_keypair, &key1.pubkey()).unwrap();
        while !bank4.is_complete() {
            bank4.register_tick(&Hash::new_unique());
        }

        let incremental_snapshot_archive_path = bank_to_incremental_snapshot_archive(
            snapshot_dir.path(),
            &bank4,
            full_snapshot_slot,
            None,
            snapshot_package_output_dir.path(),
            snapshot_archive_format,
            None,
            std::usize::MAX,
        )
        .unwrap();

        let roundtrip_bank = bank_from_incremental_snapshot_archive(
            &[PathBuf::from(accounts_dir.path())],
            &[],
            snapshot_dir.path(),
            &full_snapshot_archive_path,
            &incremental_snapshot_archive_path,
            snapshot_archive_format,
            &genesis_config,
            None,
            None,
            AccountSecondaryIndexes::default(),
            false,
            None,
            AccountShrinkThreshold::default(),
        )
        .unwrap();

        assert_eq!(*bank4, roundtrip_bank);
    }
}

use crate::bank_forks::CompressionType;
use crate::hardened_unpack::{unpack_snapshot, UnpackError};
use crate::snapshot_package::AccountsPackage;
use bincode::serialize_into;
use bzip2::bufread::BzDecoder;
use flate2::read::GzDecoder;
use fs_extra::dir::CopyOptions;
use log::*;
use regex::Regex;
use solana_measure::measure::Measure;
use solana_runtime::{
    accounts_db::{SnapshotStorage, SnapshotStorages},
    bank::{
        self, deserialize_from_snapshot, Bank, BankRcSerialize, BankSlotDelta,
        MAX_SNAPSHOT_DATA_FILE_SIZE,
    },
};
use solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey};
use std::{
    cmp::Ordering,
    fs::{self, File},
    io::{BufReader, BufWriter, Error as IOError, ErrorKind, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::ExitStatus,
};
use tar::Archive;
use tempfile::TempDir;
use thiserror::Error;

pub const SNAPSHOT_STATUS_CACHE_FILE_NAME: &str = "status_cache";
pub const TAR_SNAPSHOTS_DIR: &str = "snapshots";
pub const TAR_ACCOUNTS_DIR: &str = "accounts";
pub const TAR_VERSION_FILE: &str = "version";

pub const SNAPSHOT_VERSION: &str = "1.1.0";

#[derive(PartialEq, Ord, Eq, Debug)]
pub struct SlotSnapshotPaths {
    pub slot: Slot,
    pub snapshot_file_path: PathBuf,
}

#[derive(Error, Debug)]
pub enum SnapshotError {
    #[error("I/O error")]
    IO(#[from] std::io::Error),

    #[error("serialization error")]
    Serialize(#[from] Box<bincode::ErrorKind>),

    #[error("file system error")]
    FsExtra(#[from] fs_extra::error::Error),

    #[error("archive generation failure {0}")]
    ArchiveGenerationFailure(ExitStatus),

    #[error("storage path symlink is invalid")]
    StoragePathSymlinkInvalid,

    #[error("Unpack error")]
    UnpackError(#[from] UnpackError),
}
pub type Result<T> = std::result::Result<T, SnapshotError>;

impl PartialOrd for SlotSnapshotPaths {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.slot.cmp(&other.slot))
    }
}

impl SlotSnapshotPaths {
    fn copy_snapshot_directory<P: AsRef<Path>>(&self, snapshot_hardlink_dir: P) -> Result<()> {
        // Create a new directory in snapshot_hardlink_dir
        let new_slot_hardlink_dir = snapshot_hardlink_dir.as_ref().join(self.slot.to_string());
        let _ = fs::remove_dir_all(&new_slot_hardlink_dir);
        fs::create_dir_all(&new_slot_hardlink_dir)?;

        // Copy the snapshot
        fs::copy(
            &self.snapshot_file_path,
            &new_slot_hardlink_dir.join(self.slot.to_string()),
        )?;
        Ok(())
    }
}

pub fn package_snapshot<P: AsRef<Path>, Q: AsRef<Path>>(
    bank: &Bank,
    snapshot_files: &SlotSnapshotPaths,
    snapshot_path: Q,
    slots_to_snapshot: &[Slot],
    snapshot_package_output_path: P,
    snapshot_storages: SnapshotStorages,
    compression: CompressionType,
) -> Result<AccountsPackage> {
    // Hard link all the snapshots we need for this package
    let snapshot_hard_links_dir = tempfile::tempdir_in(snapshot_path)?;

    // Create a snapshot package
    info!(
        "Snapshot for bank: {} has {} account storage entries",
        bank.slot(),
        snapshot_storages.len()
    );

    // Any errors from this point on will cause the above AccountsPackage to drop, clearing
    // any temporary state created for the AccountsPackage (like the snapshot_hard_links_dir)
    snapshot_files.copy_snapshot_directory(snapshot_hard_links_dir.path())?;

    let snapshot_package_output_file = get_snapshot_archive_path(
        &snapshot_package_output_path,
        &(bank.slot(), bank.get_accounts_hash()),
        &compression,
    );

    let package = AccountsPackage::new(
        bank.slot(),
        bank.block_height(),
        bank.src.slot_deltas(slots_to_snapshot),
        snapshot_hard_links_dir,
        snapshot_storages,
        snapshot_package_output_file,
        bank.get_accounts_hash(),
        compression,
    );

    Ok(package)
}

fn get_compression_ext(compression: &CompressionType) -> (&'static str, &'static str) {
    match compression {
        CompressionType::Bzip2 => ("bzip2", ".tar.bz2"),
        CompressionType::Gzip => ("gzip", ".tar.gz"),
        CompressionType::Zstd => ("zstd", ".tar.zst"),
        CompressionType::NoCompression => ("", ".tar"),
    }
}

pub fn archive_snapshot_package(snapshot_package: &AccountsPackage) -> Result<()> {
    info!(
        "Generating snapshot archive for slot {}",
        snapshot_package.root
    );

    serialize_status_cache(
        snapshot_package.root,
        &snapshot_package.slot_deltas,
        &snapshot_package.snapshot_links,
    )?;

    let mut timer = Measure::start("snapshot_package-package_snapshots");
    let tar_dir = snapshot_package
        .tar_output_file
        .parent()
        .expect("Tar output path is invalid");

    fs::create_dir_all(tar_dir)?;

    // Create the staging directories
    let staging_dir = TempDir::new()?;
    let staging_accounts_dir = staging_dir.path().join(TAR_ACCOUNTS_DIR);
    let staging_snapshots_dir = staging_dir.path().join(TAR_SNAPSHOTS_DIR);
    let staging_version_file = staging_dir.path().join(TAR_VERSION_FILE);
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
        let output_path = staging_accounts_dir.join(
            storage_path
                .file_name()
                .expect("Invalid AppendVec file path"),
        );

        // `storage_path` - The file path where the AppendVec itself is located
        // `output_path` - The directory where the AppendVec will be placed in the staging directory.
        let storage_path =
            fs::canonicalize(storage_path).expect("Could not get absolute path for accounts");
        symlink::symlink_dir(storage_path, &output_path)?;
        if !output_path.is_file() {
            return Err(SnapshotError::StoragePathSymlinkInvalid);
        }
    }

    // Write version file
    {
        let mut f = std::fs::File::create(staging_version_file)?;
        f.write_all(&SNAPSHOT_VERSION.to_string().into_bytes())?;
    }

    let (compression_option, file_ext) = get_compression_ext(&snapshot_package.compression);
    let archive_options = "cfhS";

    // Tar the staging directory into the archive at `archive_path`
    let archive_file = format!("new_state{}", file_ext);
    let archive_path = tar_dir.join(archive_file);
    let args = vec![
        archive_options,
        archive_path.to_str().unwrap(),
        "--use-compress-program",
        compression_option,
        "-C",
        staging_dir.path().to_str().unwrap(),
        TAR_ACCOUNTS_DIR,
        TAR_SNAPSHOTS_DIR,
        TAR_VERSION_FILE,
    ];

    let output = std::process::Command::new("tar").args(&args).output()?;
    if !output.status.success() {
        warn!("tar command failed with exit code: {}", output.status);
        use std::str::from_utf8;
        info!("tar stdout: {}", from_utf8(&output.stdout).unwrap_or("?"));
        info!("tar stderr: {}", from_utf8(&output.stderr).unwrap_or("?"));

        return Err(SnapshotError::ArchiveGenerationFailure(output.status));
    }

    // Once everything is successful, overwrite the previous tarball so that other validators
    // can fetch this newly packaged snapshot
    let metadata = fs::metadata(&archive_path)?;
    fs::rename(&archive_path, &snapshot_package.tar_output_file)?;

    // Keep around at most two snapshot archives
    let archives = get_snapshot_archives(snapshot_package.tar_output_file.parent().unwrap());
    for old_archive in archives.into_iter().skip(2) {
        fs::remove_file(old_archive.0)
            .unwrap_or_else(|err| info!("Failed to remove old snapshot: {:}", err));
    }

    timer.stop();
    info!(
        "Successfully created {:?}. slot: {}, elapsed ms: {}, size={}",
        snapshot_package.tar_output_file,
        snapshot_package.root,
        timer.as_ms(),
        metadata.len()
    );
    datapoint_info!(
        "snapshot-package",
        ("slot", snapshot_package.root, i64),
        ("duration_ms", timer.as_ms(), i64),
        ("size", metadata.len(), i64)
    );
    Ok(())
}

pub fn get_snapshot_paths<P: AsRef<Path>>(snapshot_path: P) -> Vec<SlotSnapshotPaths>
where
    P: std::fmt::Debug,
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

pub fn serialize_snapshot_data_file<F>(
    data_file_path: &Path,
    maximum_file_size: u64,
    mut serializer: F,
) -> Result<u64>
where
    F: FnMut(&mut BufWriter<File>) -> Result<()>,
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

pub fn deserialize_snapshot_data_file<F, T>(
    data_file_path: &Path,
    maximum_file_size: u64,
    mut deserializer: F,
) -> Result<T>
where
    F: FnMut(&mut BufReader<File>) -> Result<T>,
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

pub fn add_snapshot<P: AsRef<Path>>(
    snapshot_path: P,
    bank: &Bank,
    snapshot_storages: &[SnapshotStorage],
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
    let consumed_size = serialize_snapshot_data_file(
        &snapshot_bank_file_path,
        MAX_SNAPSHOT_DATA_FILE_SIZE,
        |stream| {
            serialize_into(stream.by_ref(), &*bank)?;
            serialize_into(
                stream.by_ref(),
                &BankRcSerialize {
                    bank_rc: &bank.rc,
                    snapshot_storages,
                },
            )?;
            Ok(())
        },
    )?;
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

pub fn serialize_status_cache(
    slot: Slot,
    slot_deltas: &[BankSlotDelta],
    snapshot_links: &TempDir,
) -> Result<()> {
    // the status cache is stored as snapshot_path/status_cache
    let snapshot_status_cache_file_path =
        snapshot_links.path().join(SNAPSHOT_STATUS_CACHE_FILE_NAME);

    let mut status_cache_serialize = Measure::start("status_cache_serialize-ms");
    let consumed_size = serialize_snapshot_data_file(
        &snapshot_status_cache_file_path,
        MAX_SNAPSHOT_DATA_FILE_SIZE,
        |stream| {
            serialize_into(stream, slot_deltas)?;
            Ok(())
        },
    )?;
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

pub fn bank_from_archive<P: AsRef<Path>>(
    account_paths: &[PathBuf],
    frozen_account_pubkeys: &[Pubkey],
    snapshot_path: &PathBuf,
    snapshot_tar: P,
    compression: CompressionType,
) -> Result<Bank> {
    // Untar the snapshot into a temp directory under `snapshot_config.snapshot_path()`
    let unpack_dir = tempfile::tempdir_in(snapshot_path)?;
    untar_snapshot_in(&snapshot_tar, &unpack_dir, compression)?;

    let mut measure = Measure::start("bank rebuild from snapshot");
    let unpacked_accounts_dir = unpack_dir.as_ref().join(TAR_ACCOUNTS_DIR);
    let unpacked_snapshots_dir = unpack_dir.as_ref().join(TAR_SNAPSHOTS_DIR);
    let unpacked_version_file = unpack_dir.as_ref().join(TAR_VERSION_FILE);

    let mut snapshot_version = String::new();
    File::open(unpacked_version_file).and_then(|mut f| f.read_to_string(&mut snapshot_version))?;

    let bank = rebuild_bank_from_snapshots(
        snapshot_version.trim(),
        account_paths,
        frozen_account_pubkeys,
        &unpacked_snapshots_dir,
        unpacked_accounts_dir,
    )?;

    if !bank.verify_snapshot_bank() {
        panic!("Snapshot bank for slot {} failed to verify", bank.slot());
    }
    measure.stop();
    info!("{}", measure);

    // Move the unpacked snapshots into `snapshot_path`
    let dir_files = fs::read_dir(&unpacked_snapshots_dir).unwrap_or_else(|err| {
        panic!(
            "Invalid snapshot path {:?}: {}",
            unpacked_snapshots_dir, err
        )
    });
    let paths: Vec<PathBuf> = dir_files
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    let mut copy_options = CopyOptions::new();
    copy_options.overwrite = true;
    fs_extra::move_items(&paths, &snapshot_path, &copy_options)?;

    Ok(bank)
}

pub fn get_snapshot_archive_path<P: AsRef<Path>>(
    snapshot_output_dir: P,
    snapshot_hash: &(Slot, Hash),
    compression: &CompressionType,
) -> PathBuf {
    snapshot_output_dir.as_ref().join(format!(
        "snapshot-{}-{}{}",
        snapshot_hash.0,
        snapshot_hash.1,
        get_compression_ext(compression).1,
    ))
}

fn compression_type_from_str(compress: &str) -> Option<CompressionType> {
    match compress {
        "bz2" => Some(CompressionType::Bzip2),
        "gz" => Some(CompressionType::Gzip),
        "zst" => Some(CompressionType::Zstd),
        _ => None,
    }
}

fn snapshot_hash_of(archive_filename: &str) -> Option<(Slot, Hash, CompressionType)> {
    let snapshot_filename_regex =
        Regex::new(r"snapshot-(\d+)-([[:alnum:]]+)\.tar\.(bz2|zst|gz)$").unwrap();

    if let Some(captures) = snapshot_filename_regex.captures(archive_filename) {
        let slot_str = captures.get(1).unwrap().as_str();
        let hash_str = captures.get(2).unwrap().as_str();
        let ext = captures.get(3).unwrap().as_str();

        if let (Ok(slot), Ok(hash), Some(compression)) = (
            slot_str.parse::<Slot>(),
            hash_str.parse::<Hash>(),
            compression_type_from_str(ext),
        ) {
            return Some((slot, hash, compression));
        }
    }
    None
}

pub fn get_snapshot_archives<P: AsRef<Path>>(
    snapshot_output_dir: P,
) -> Vec<(PathBuf, (Slot, Hash, CompressionType))> {
    match fs::read_dir(&snapshot_output_dir) {
        Err(err) => {
            info!("Unable to read snapshot directory: {}", err);
            vec![]
        }
        Ok(files) => {
            let mut archives: Vec<_> = files
                .filter_map(|entry| {
                    if let Ok(entry) = entry {
                        let path = entry.path();
                        if path.is_file() {
                            if let Some(snapshot_hash) =
                                snapshot_hash_of(path.file_name().unwrap().to_str().unwrap())
                            {
                                return Some((path, snapshot_hash));
                            }
                        }
                    }
                    None
                })
                .collect();

            archives.sort_by(|a, b| (b.1).0.cmp(&(a.1).0)); // reverse sort by slot
            archives
        }
    }
}

pub fn get_highest_snapshot_archive_path<P: AsRef<Path>>(
    snapshot_output_dir: P,
) -> Option<(PathBuf, (Slot, Hash, CompressionType))> {
    let archives = get_snapshot_archives(snapshot_output_dir);
    archives.into_iter().next()
}

pub fn untar_snapshot_in<P: AsRef<Path>, Q: AsRef<Path>>(
    snapshot_tar: P,
    unpack_dir: Q,
    compression: CompressionType,
) -> Result<()> {
    let mut measure = Measure::start("snapshot untar");
    let tar_name = File::open(&snapshot_tar)?;
    match compression {
        CompressionType::Bzip2 => {
            let tar = BzDecoder::new(BufReader::new(tar_name));
            let mut archive = Archive::new(tar);
            unpack_snapshot(&mut archive, unpack_dir)?;
        }
        CompressionType::Gzip => {
            let tar = GzDecoder::new(BufReader::new(tar_name));
            let mut archive = Archive::new(tar);
            unpack_snapshot(&mut archive, unpack_dir)?;
        }
        CompressionType::Zstd => {
            let tar = zstd::stream::read::Decoder::new(BufReader::new(tar_name))?;
            let mut archive = Archive::new(tar);
            unpack_snapshot(&mut archive, unpack_dir)?;
        }
        CompressionType::NoCompression => {
            let tar = BufReader::new(tar_name);
            let mut archive = Archive::new(tar);
            unpack_snapshot(&mut archive, unpack_dir)?;
        }
    };
    measure.stop();
    info!("{}", measure);
    Ok(())
}

fn rebuild_bank_from_snapshots<P>(
    snapshot_version: &str,
    account_paths: &[PathBuf],
    frozen_account_pubkeys: &[Pubkey],
    unpacked_snapshots_dir: &PathBuf,
    append_vecs_path: P,
) -> Result<Bank>
where
    P: AsRef<Path>,
{
    info!("snapshot version: {}", snapshot_version);

    let mut snapshot_paths = get_snapshot_paths(&unpacked_snapshots_dir);
    if snapshot_paths.len() > 1 {
        return Err(get_io_error("invalid snapshot format"));
    }
    let root_paths = snapshot_paths
        .pop()
        .ok_or_else(|| get_io_error("No snapshots found in snapshots directory"))?;

    info!("Loading bank from {:?}", &root_paths.snapshot_file_path);
    let bank = deserialize_snapshot_data_file(
        &root_paths.snapshot_file_path,
        MAX_SNAPSHOT_DATA_FILE_SIZE,
        |stream| {
            let mut bank: Bank = match snapshot_version {
                SNAPSHOT_VERSION => deserialize_from_snapshot(stream.by_ref())?,
                _ => {
                    return Err(get_io_error(&format!(
                        "unsupported snapshot version: {}",
                        snapshot_version
                    )));
                }
            };
            info!("Rebuilding accounts...");
            let rc = bank::BankRc::from_stream(
                account_paths,
                bank.slot(),
                &bank.ancestors,
                frozen_account_pubkeys,
                stream.by_ref(),
                &append_vecs_path,
            )?;
            bank.set_bank_rc(rc, bank::StatusCacheRc::default());
            bank.finish_init();
            Ok(bank)
        },
    )?;

    let status_cache_path = unpacked_snapshots_dir.join(SNAPSHOT_STATUS_CACHE_FILE_NAME);
    let slot_deltas = deserialize_snapshot_data_file(
        &status_cache_path,
        MAX_SNAPSHOT_DATA_FILE_SIZE,
        |stream| {
            info!("Rebuilding status cache...");
            let slot_deltas: Vec<BankSlotDelta> = deserialize_from_snapshot(stream)?;
            Ok(slot_deltas)
        },
    )?;

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
    SnapshotError::IO(IOError::new(ErrorKind::Other, error))
}

pub fn verify_snapshot_archive<P, Q, R>(
    snapshot_archive: P,
    snapshots_to_verify: Q,
    storages_to_verify: R,
    compression: CompressionType,
) where
    P: AsRef<Path>,
    Q: AsRef<Path>,
    R: AsRef<Path>,
{
    let temp_dir = tempfile::TempDir::new().unwrap();
    let unpack_dir = temp_dir.path();
    untar_snapshot_in(snapshot_archive, &unpack_dir, compression).unwrap();

    // Check snapshots are the same
    let unpacked_snapshots = unpack_dir.join(&TAR_SNAPSHOTS_DIR);
    assert!(!dir_diff::is_different(&snapshots_to_verify, unpacked_snapshots).unwrap());

    // Check the account entries are the same
    let unpacked_accounts = unpack_dir.join(&TAR_ACCOUNTS_DIR);
    assert!(!dir_diff::is_different(&storages_to_verify, unpacked_accounts).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize_from, serialize_into};
    use matches::assert_matches;
    use std::mem::size_of;

    #[test]
    fn test_serialize_snapshot_data_file_under_limit() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let expected_consumed_size = size_of::<u32>() as u64;
        let consumed_size = serialize_snapshot_data_file(
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
        let result = serialize_snapshot_data_file(
            &temp_dir.path().join("data-file"),
            expected_consumed_size - 1,
            |stream| {
                serialize_into(stream, &2323_u32)?;
                Ok(())
            },
        );
        assert_matches!(result, Err(SnapshotError::IO(ref message)) if message.to_string().starts_with("too large snapshot data file to serialize"));
    }

    #[test]
    fn test_deserialize_snapshot_data_file_under_limit() {
        let expected_data = 2323_u32;
        let expected_consumed_size = size_of::<u32>() as u64;

        let temp_dir = tempfile::TempDir::new().unwrap();
        serialize_snapshot_data_file(
            &temp_dir.path().join("data-file"),
            expected_consumed_size,
            |stream| {
                serialize_into(stream, &expected_data)?;
                Ok(())
            },
        )
        .unwrap();

        let actual_data = deserialize_snapshot_data_file(
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
        serialize_snapshot_data_file(
            &temp_dir.path().join("data-file"),
            expected_consumed_size,
            |stream| {
                serialize_into(stream, &expected_data)?;
                Ok(())
            },
        )
        .unwrap();

        let result = deserialize_snapshot_data_file(
            &temp_dir.path().join("data-file"),
            expected_consumed_size - 1,
            |stream| Ok(deserialize_from::<_, u32>(stream)?),
        );
        assert_matches!(result, Err(SnapshotError::IO(ref message)) if message.to_string().starts_with("too large snapshot data file to deserialize"));
    }

    #[test]
    fn test_deserialize_snapshot_data_file_extra_data() {
        let expected_data = 2323_u32;
        let expected_consumed_size = size_of::<u32>() as u64;

        let temp_dir = tempfile::TempDir::new().unwrap();
        serialize_snapshot_data_file(
            &temp_dir.path().join("data-file"),
            expected_consumed_size * 2,
            |stream| {
                serialize_into(stream.by_ref(), &expected_data)?;
                serialize_into(stream.by_ref(), &expected_data)?;
                Ok(())
            },
        )
        .unwrap();

        let result = deserialize_snapshot_data_file(
            &temp_dir.path().join("data-file"),
            expected_consumed_size * 2,
            |stream| Ok(deserialize_from::<_, u32>(stream)?),
        );
        assert_matches!(result, Err(SnapshotError::IO(ref message)) if message.to_string().starts_with("invalid snapshot data file"));
    }

    #[test]
    fn test_snapshot_hash_of() {
        assert_eq!(
            snapshot_hash_of(&format!("snapshot-42-{}.tar.bz2", Hash::default())),
            Some((42, Hash::default(), CompressionType::Bzip2))
        );
        assert_eq!(
            snapshot_hash_of(&format!("snapshot-43-{}.tar.zst", Hash::default())),
            Some((43, Hash::default(), CompressionType::Zstd))
        );

        assert!(snapshot_hash_of("invalid").is_none());
    }
}

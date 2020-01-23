use crate::snapshot_package::SnapshotPackage;
use bincode::serialize_into;
use bzip2::bufread::BzDecoder;
use fs_extra::dir::CopyOptions;
use log::*;
use solana_measure::measure::Measure;
use solana_runtime::{
    bank::{self, deserialize_from_snapshot, Bank, MAX_SNAPSHOT_DATA_FILE_SIZE},
    status_cache::SlotDelta,
};
use solana_sdk::transaction::Result as TransactionResult;
use solana_sdk::{clock::Slot, transaction};
use std::{
    cmp::Ordering,
    fs,
    fs::File,
    io::{BufReader, BufWriter, Error as IOError, ErrorKind, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use tar::Archive;
use tempfile::TempDir;
use thiserror::Error;

pub const SNAPSHOT_STATUS_CACHE_FILE_NAME: &str = "status_cache";
pub const TAR_SNAPSHOTS_DIR: &str = "snapshots";
pub const TAR_ACCOUNTS_DIR: &str = "accounts";
pub const TAR_VERSION_FILE: &str = "version";

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
    snapshot_package_output_file: P,
    snapshot_path: Q,
    slots_to_snapshot: &[Slot],
) -> Result<SnapshotPackage> {
    // Hard link all the snapshots we need for this package
    let snapshot_hard_links_dir = tempfile::tempdir_in(snapshot_path)?;

    // Get a reference to all the relevant AccountStorageEntries
    let account_storage_entries: Vec<_> = bank
        .rc
        .get_rooted_storage_entries()
        .into_iter()
        .filter(|x| x.slot_id() <= bank.slot())
        .collect();

    // Create a snapshot package
    info!(
        "Snapshot for bank: {} has {} account storage entries",
        bank.slot(),
        account_storage_entries.len()
    );

    // Any errors from this point on will cause the above SnapshotPackage to drop, clearing
    // any temporary state created for the SnapshotPackage (like the snapshot_hard_links_dir)
    snapshot_files.copy_snapshot_directory(snapshot_hard_links_dir.path())?;

    let package = SnapshotPackage::new(
        bank.slot(),
        bank.src.slot_deltas(slots_to_snapshot),
        snapshot_hard_links_dir,
        account_storage_entries,
        snapshot_package_output_file.as_ref().to_path_buf(),
    );

    Ok(package)
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

pub fn add_snapshot<P: AsRef<Path>>(snapshot_path: P, bank: &Bank) -> Result<SlotSnapshotPaths> {
    bank.purge_zero_lamport_accounts();
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
            serialize_into(stream.by_ref(), &bank.rc)?;
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
    slot_deltas: &[SlotDelta<TransactionResult<()>>],
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

pub fn bank_slot_from_archive<P: AsRef<Path>>(snapshot_tar: P) -> Result<Slot> {
    let tempdir = tempfile::TempDir::new()?;
    untar_snapshot_in(&snapshot_tar, &tempdir)?;
    let unpacked_snapshots_dir = tempdir.path().join(TAR_SNAPSHOTS_DIR);
    let local_account_paths = vec![tempdir.path().join("account_dummy")];
    let unpacked_accounts_dir = tempdir.path().join(TAR_ACCOUNTS_DIR);
    let snapshot_paths = get_snapshot_paths(&unpacked_snapshots_dir);
    let last_root_paths = snapshot_paths
        .last()
        .ok_or_else(|| get_io_error("No snapshots found in snapshots directory"))?;
    let bank = deserialize_snapshot_data_file(
        &last_root_paths.snapshot_file_path,
        MAX_SNAPSHOT_DATA_FILE_SIZE,
        |stream| {
            let bank: Bank = deserialize_from_snapshot(stream.by_ref())?;
            bank.rc.accounts_from_stream(
                stream.by_ref(),
                &local_account_paths,
                &unpacked_accounts_dir,
            )?;
            Ok(bank)
        },
    )?;
    Ok(bank.slot())
}

pub fn bank_from_archive<P: AsRef<Path>>(
    account_paths: &[PathBuf],
    snapshot_path: &PathBuf,
    snapshot_tar: P,
) -> Result<Bank> {
    // Untar the snapshot into a temp directory under `snapshot_config.snapshot_path()`
    let unpack_dir = tempfile::tempdir_in(snapshot_path)?;
    untar_snapshot_in(&snapshot_tar, &unpack_dir)?;

    let mut measure = Measure::start("bank rebuild from snapshot");
    let unpacked_accounts_dir = unpack_dir.as_ref().join(TAR_ACCOUNTS_DIR);
    let unpacked_snapshots_dir = unpack_dir.as_ref().join(TAR_SNAPSHOTS_DIR);
    let unpacked_version_file = unpack_dir.as_ref().join(TAR_VERSION_FILE);

    let snapshot_version = if let Ok(mut f) = File::open(unpacked_version_file) {
        let mut snapshot_version = String::new();
        f.read_to_string(&mut snapshot_version)?;
        snapshot_version
    } else {
        // Once v0.23.x is deployed, this default can be removed and snapshots without a version
        // file can be rejected
        String::from("v0.22.3")
    };

    let bank = rebuild_bank_from_snapshots(
        snapshot_version.trim(),
        account_paths,
        &unpacked_snapshots_dir,
        unpacked_accounts_dir,
    )?;

    if !bank.verify_snapshot_bank() {
        panic!("Snapshot bank failed to verify");
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

pub fn get_snapshot_tar_path<P: AsRef<Path>>(snapshot_output_dir: P) -> PathBuf {
    snapshot_output_dir.as_ref().join("snapshot.tar.bz2")
}

pub fn untar_snapshot_in<P: AsRef<Path>, Q: AsRef<Path>>(
    snapshot_tar: P,
    unpack_dir: Q,
) -> Result<()> {
    let mut measure = Measure::start("snapshot untar");
    let tar_bz2 = File::open(snapshot_tar)?;
    let tar = BzDecoder::new(BufReader::new(tar_bz2));
    let mut archive = Archive::new(tar);
    archive.unpack(&unpack_dir)?;
    measure.stop();
    info!("{}", measure);
    Ok(())
}

fn rebuild_bank_from_snapshots<P>(
    snapshot_version: &str,
    account_paths: &[PathBuf],
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
                env!("CARGO_PKG_VERSION") => deserialize_from_snapshot(stream.by_ref())?,
                "v0.22.3" => {
                    let bank0223: solana_runtime::bank::LegacyBank0223 =
                        deserialize_from_snapshot(stream.by_ref())?;
                    bank0223.into()
                }
                _ => {
                    return Err(get_io_error(&format!(
                        "unsupported snapshot version: {}",
                        snapshot_version
                    )));
                }
            };
            // Rebuild accounts
            bank.set_bank_rc(
                bank::BankRc::new(account_paths.to_vec(), 0, bank.slot()),
                bank::StatusCacheRc::default(),
            );
            bank.rc
                .accounts_from_stream(stream.by_ref(), account_paths, &append_vecs_path)?;
            Ok(bank)
        },
    )?;

    let status_cache_path = unpacked_snapshots_dir.join(SNAPSHOT_STATUS_CACHE_FILE_NAME);
    let slot_deltas = deserialize_snapshot_data_file(
        &status_cache_path,
        MAX_SNAPSHOT_DATA_FILE_SIZE,
        |stream| {
            // Rebuild status cache
            let slot_deltas: Vec<SlotDelta<transaction::Result<()>>> =
                deserialize_from_snapshot(stream)?;

            Ok(slot_deltas)
        },
    )?;

    bank.src.append(&slot_deltas);

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

pub fn verify_snapshot_tar<P, Q, R>(snapshot_tar: P, snapshots_to_verify: Q, storages_to_verify: R)
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
    R: AsRef<Path>,
{
    let temp_dir = tempfile::TempDir::new().unwrap();
    let unpack_dir = temp_dir.path();
    untar_snapshot_in(snapshot_tar, &unpack_dir).unwrap();

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
}

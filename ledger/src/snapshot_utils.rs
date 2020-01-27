use crate::snapshot_package::SnapshotPackage;
use bincode::{deserialize_from, serialize_into};
use bzip2::bufread::BzDecoder;
use fs_extra::dir::CopyOptions;
use log::*;
use solana_measure::measure::Measure;
use solana_runtime::{
    bank::{self, Bank},
    status_cache::SlotDelta,
};
use solana_sdk::{clock::Slot, transaction};
use std::{
    cmp::Ordering,
    fs::{self, File},
    io::{BufReader, BufWriter, Error as IOError, ErrorKind, Read, Write},
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

pub fn archive_snapshot_package(snapshot_package: &SnapshotPackage) -> Result<()> {
    info!(
        "Generating snapshot tarball for root {}",
        snapshot_package.root
    );

    fn serialize_status_cache(
        slot_deltas: &[SlotDelta<transaction::Result<()>>],
        snapshot_links: &TempDir,
    ) -> Result<()> {
        // the status cache is stored as snapshot_path/status_cache
        let snapshot_status_cache_file_path =
            snapshot_links.path().join(SNAPSHOT_STATUS_CACHE_FILE_NAME);

        let status_cache = File::create(&snapshot_status_cache_file_path)?;
        // status cache writer
        let mut status_cache_stream = BufWriter::new(status_cache);

        let mut status_cache_serialize = Measure::start("status_cache_serialize-ms");
        // write the status cache
        serialize_into(&mut status_cache_stream, slot_deltas)
            .map_err(|_| get_io_error("serialize status cache error"))?;
        status_cache_serialize.stop();
        inc_new_counter_info!(
            "serialize-status-cache-ms",
            status_cache_serialize.as_ms() as usize
        );
        Ok(())
    }

    serialize_status_cache(
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
    for storage in &snapshot_package.storage_entries {
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
            return Err(get_io_error(
                "Error trying to generate snapshot archive: storage path symlink is invalid",
            ));
        }
    }

    // Write version file
    {
        let snapshot_version = format!("{}\n", env!("CARGO_PKG_VERSION"));
        let mut f = std::fs::File::create(staging_version_file)?;
        //f.write_all(&snapshot_version.to_string().into_bytes())?;
        f.write_all(&snapshot_version.into_bytes())?;
    }

    // Tar the staging directory into the archive at `archive_path`
    let archive_path = tar_dir.join("new_state.tar.bz2");
    let args = vec![
        "jcfhS",
        archive_path.to_str().unwrap(),
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

        return Err(get_io_error(&format!(
            "Error trying to generate snapshot archive: {}",
            output.status
        )));
    }

    // Once everything is successful, overwrite the previous tarball so that other validators
    // can fetch this newly packaged snapshot
    let metadata = fs::metadata(&archive_path)?;
    fs::rename(&archive_path, &snapshot_package.tar_output_file)?;

    timer.stop();
    info!(
        "Successfully created tarball. slot: {}, elapsed ms: {}, size={}",
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

pub fn add_snapshot<P: AsRef<Path>>(snapshot_path: P, bank: &Bank) -> Result<SlotSnapshotPaths> {
    bank.purge_zero_lamport_accounts();
    let slot = bank.slot();
    // snapshot_path/slot
    let slot_snapshot_dir = get_bank_snapshot_dir(snapshot_path, slot);
    fs::create_dir_all(slot_snapshot_dir.clone())?;

    // the snapshot is stored as snapshot_path/slot/slot
    let snapshot_file_path = slot_snapshot_dir.join(get_snapshot_file_name(slot));
    info!("Creating snapshot {}, path: {:?}", slot, snapshot_file_path);

    let snapshot_file = File::create(&snapshot_file_path)?;
    // snapshot writer
    let mut snapshot_stream = BufWriter::new(snapshot_file);
    // Create the snapshot
    serialize_into(&mut snapshot_stream, &*bank)?;
    let mut bank_rc_serialize = Measure::start("create snapshot");
    serialize_into(&mut snapshot_stream, &bank.rc)?;
    bank_rc_serialize.stop();
    inc_new_counter_info!("bank-rc-serialize-ms", bank_rc_serialize.as_ms() as usize);

    info!(
        "{} for slot {} at {:?}",
        bank_rc_serialize, slot, snapshot_file_path,
    );

    Ok(SlotSnapshotPaths {
        slot,
        snapshot_file_path,
    })
}

pub fn remove_snapshot<P: AsRef<Path>>(slot: Slot, snapshot_path: P) -> Result<()> {
    let slot_snapshot_dir = get_bank_snapshot_dir(&snapshot_path, slot);
    // Remove the snapshot directory for this slot
    fs::remove_dir_all(slot_snapshot_dir)?;
    Ok(())
}

pub fn bank_slot_from_archive<P: AsRef<Path>>(snapshot_tar: P) -> Result<u64> {
    let tempdir = tempfile::TempDir::new()?;
    untar_snapshot_in(&snapshot_tar, &tempdir)?;
    let unpacked_snapshots_dir = tempdir.path().join(TAR_SNAPSHOTS_DIR);
    let snapshot_paths = get_snapshot_paths(&unpacked_snapshots_dir);
    let last_root_paths = snapshot_paths
        .last()
        .ok_or_else(|| get_io_error("No snapshots found in snapshots directory"))?;
    let file = File::open(&last_root_paths.snapshot_file_path)?;
    let mut stream = BufReader::new(file);
    let bank: Bank = deserialize_from(&mut stream)?;
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
        // Once 0.23.x is deployed, this default can be removed and snapshots without a version
        // file can be rejected
        String::from("0.22.3")
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

pub fn get_snapshot_archive_path<P: AsRef<Path>>(snapshot_output_dir: P) -> PathBuf {
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

    // Rebuild the root bank
    info!("Loading bank from {:?}", &root_paths.snapshot_file_path);
    let file = File::open(&root_paths.snapshot_file_path)?;
    let mut stream = BufReader::new(file);
    let mut bank: Bank = match snapshot_version {
        env!("CARGO_PKG_VERSION") => deserialize_from(&mut stream)?,
        "0.22.3" => {
            let bank0223: solana_runtime::bank::LegacyBank0223 = deserialize_from(&mut stream)?;
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
        .accounts_from_stream(&mut stream, account_paths, append_vecs_path)?;

    // Rebuild status cache
    let status_cache_path = unpacked_snapshots_dir.join(SNAPSHOT_STATUS_CACHE_FILE_NAME);
    let status_cache = File::open(status_cache_path)?;
    let mut stream = BufReader::new(status_cache);
    let slot_deltas: Vec<SlotDelta<transaction::Result<()>>> =
        deserialize_from(&mut stream).unwrap_or_default();

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

pub fn verify_snapshot_archive<P, Q, R>(
    snapshot_tar: P,
    snapshots_to_verify: Q,
    storages_to_verify: R,
) where
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

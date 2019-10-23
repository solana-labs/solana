use crate::snapshot_package::SnapshotPackage;
use bincode::{deserialize_from, serialize_into};
use bzip2::bufread::BzDecoder;
use fs_extra::dir::CopyOptions;
use log::*;
use solana_measure::measure::Measure;
use solana_runtime::bank::Bank;
use solana_runtime::status_cache::SlotDelta;
use solana_sdk::transaction;
use std::cmp::Ordering;
use std::fs;
use std::fs::File;
use std::io::{BufReader, BufWriter, Error as IOError, ErrorKind};
use std::path::{Path, PathBuf};
use tar::Archive;

pub const SNAPSHOT_STATUS_CACHE_FILE_NAME: &str = "status_cache";
pub const TAR_SNAPSHOTS_DIR: &str = "snapshots";
pub const TAR_ACCOUNTS_DIR: &str = "accounts";

#[derive(PartialEq, Ord, Eq, Debug)]
pub struct SlotSnapshotPaths {
    pub slot: u64,
    pub snapshot_file_path: PathBuf,
}

#[derive(Debug)]
pub enum SnapshotError {
    IO(std::io::Error),
    Serialize(std::boxed::Box<bincode::ErrorKind>),
    FsExtra(fs_extra::error::Error),
}
pub type Result<T> = std::result::Result<T, SnapshotError>;

impl std::convert::From<std::io::Error> for SnapshotError {
    fn from(e: std::io::Error) -> SnapshotError {
        SnapshotError::IO(e)
    }
}

impl std::convert::From<std::boxed::Box<bincode::ErrorKind>> for SnapshotError {
    fn from(e: std::boxed::Box<bincode::ErrorKind>) -> SnapshotError {
        SnapshotError::Serialize(e)
    }
}

impl std::convert::From<fs_extra::error::Error> for SnapshotError {
    fn from(e: fs_extra::error::Error) -> SnapshotError {
        SnapshotError::FsExtra(e)
    }
}

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
    slots_to_snapshot: &[u64],
) -> Result<SnapshotPackage> {
    // Hard link all the snapshots we need for this package
    let snapshot_hard_links_dir = tempfile::tempdir_in(snapshot_path)?;

    // Get a reference to all the relevant AccountStorageEntries
    let account_storage_entries: Vec<_> = bank
        .rc
        .get_storage_entries()
        .into_iter()
        .filter(|x| x.fork_id() <= bank.slot())
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

pub fn add_snapshot<P: AsRef<Path>>(snapshot_path: P, bank: &Bank) -> Result<()> {
    bank.purge_zero_lamport_accounts();
    let slot = bank.slot();
    // snapshot_path/slot
    let slot_snapshot_dir = get_bank_snapshot_dir(snapshot_path, slot);
    fs::create_dir_all(slot_snapshot_dir.clone())?;

    // the snapshot is stored as snapshot_path/slot/slot
    let snapshot_file_path = slot_snapshot_dir.join(get_snapshot_file_name(slot));
    info!(
        "creating snapshot {}, path: {:?}",
        bank.slot(),
        snapshot_file_path,
    );

    let snapshot_file = File::create(&snapshot_file_path)?;
    // snapshot writer
    let mut snapshot_stream = BufWriter::new(snapshot_file);
    // Create the snapshot
    serialize_into(&mut snapshot_stream, &*bank)?;
    let mut bank_rc_serialize = Measure::start("bank_rc_serialize-ms");
    serialize_into(&mut snapshot_stream, &bank.rc)?;
    bank_rc_serialize.stop();
    inc_new_counter_info!("bank-rc-serialize-ms", bank_rc_serialize.as_ms() as usize);

    info!(
        "successfully created snapshot {}, path: {:?}",
        bank.slot(),
        snapshot_file_path,
    );

    Ok(())
}

pub fn remove_snapshot<P: AsRef<Path>>(slot: u64, snapshot_path: P) -> Result<()> {
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
    account_paths: String,
    snapshot_path: &PathBuf,
    snapshot_tar: P,
) -> Result<Bank> {
    // Untar the snapshot into a temp directory under `snapshot_config.snapshot_path()`
    let unpack_dir = tempfile::tempdir_in(snapshot_path)?;
    untar_snapshot_in(&snapshot_tar, &unpack_dir)?;

    let unpacked_accounts_dir = unpack_dir.as_ref().join(TAR_ACCOUNTS_DIR);
    let unpacked_snapshots_dir = unpack_dir.as_ref().join(TAR_SNAPSHOTS_DIR);
    let bank = rebuild_bank_from_snapshots(
        account_paths,
        &unpacked_snapshots_dir,
        unpacked_accounts_dir,
    )?;

    if !bank.verify_snapshot_bank() {
        panic!("Snapshot bank failed to verify");
    }

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
    let tar_bz2 = File::open(snapshot_tar)?;
    let tar = BzDecoder::new(BufReader::new(tar_bz2));
    let mut archive = Archive::new(tar);
    archive.unpack(&unpack_dir)?;
    Ok(())
}

fn rebuild_bank_from_snapshots<P>(
    local_account_paths: String,
    unpacked_snapshots_dir: &PathBuf,
    append_vecs_path: P,
) -> Result<Bank>
where
    P: AsRef<Path>,
{
    let mut snapshot_paths = get_snapshot_paths(&unpacked_snapshots_dir);
    if snapshot_paths.len() > 1 {
        return Err(get_io_error("invalid snapshot format"));
    }
    let root_paths = snapshot_paths
        .pop()
        .ok_or_else(|| get_io_error("No snapshots found in snapshots directory"))?;

    // Rebuild the root bank
    info!("Loading from {:?}", &root_paths.snapshot_file_path);
    let file = File::open(&root_paths.snapshot_file_path)?;
    let mut stream = BufReader::new(file);
    let bank: Bank = deserialize_from(&mut stream)?;

    // Rebuild accounts
    bank.rc
        .accounts_from_stream(&mut stream, local_account_paths, append_vecs_path)?;

    // Rebuild status cache
    let status_cache_path = unpacked_snapshots_dir.join(SNAPSHOT_STATUS_CACHE_FILE_NAME);
    let status_cache = File::open(status_cache_path)?;
    let mut stream = BufReader::new(status_cache);
    let slot_deltas: Vec<SlotDelta<transaction::Result<()>>> =
        deserialize_from(&mut stream).unwrap_or_default();

    bank.src.append(&slot_deltas);

    Ok(bank)
}

fn get_snapshot_file_name(slot: u64) -> String {
    slot.to_string()
}

fn get_bank_snapshot_dir<P: AsRef<Path>>(path: P, slot: u64) -> PathBuf {
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

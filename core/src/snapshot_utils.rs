use crate::result::{Error, Result};
use crate::snapshot_package::SnapshotPackage;
use bincode::{deserialize_from, serialize_into};
use flate2::read::GzDecoder;
use solana_runtime::bank::Bank;
use solana_runtime::status_cache::SlotDelta;
use solana_sdk::transaction;
use std::cmp::Ordering;
use std::fs;
use std::fs::File;
use std::io::{BufReader, BufWriter, Error as IOError, ErrorKind};
use std::path::{Path, PathBuf};
use tar::Archive;

const SNAPSHOT_STATUS_CACHE_FILE_NAME: &str = "status_cache";

#[derive(PartialEq, Ord, Eq, Debug)]
pub struct SlotSnapshotPaths {
    pub slot: u64,
    pub snapshot_file_path: PathBuf,
    pub snapshot_status_cache_path: PathBuf,
}

impl PartialOrd for SlotSnapshotPaths {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.slot.cmp(&other.slot))
    }
}

impl SlotSnapshotPaths {
    fn hardlink_snapshot_directory<P: AsRef<Path>>(&self, snapshot_hardlink_dir: P) -> Result<()> {
        // Create a new directory in snapshot_hardlink_dir
        let new_slot_hardlink_dir = snapshot_hardlink_dir.as_ref().join(self.slot.to_string());
        let _ = fs::remove_dir_all(&new_slot_hardlink_dir);
        fs::create_dir_all(&new_slot_hardlink_dir)?;

        // Hardlink the snapshot
        fs::hard_link(
            &self.snapshot_file_path,
            &new_slot_hardlink_dir.join(self.slot.to_string()),
        )?;
        // Hardlink the status cache
        fs::hard_link(
            &self.snapshot_status_cache_path,
            &new_slot_hardlink_dir.join(SNAPSHOT_STATUS_CACHE_FILE_NAME),
        )?;
        Ok(())
    }
}

pub fn package_snapshot<Q: AsRef<Path>>(
    bank: &Bank,
    snapshot_files: &[SlotSnapshotPaths],
    snapshot_package_output_file: Q,
) -> Result<SnapshotPackage> {
    let slot = bank.slot();

    // Hard link all the snapshots we need for this package
    let snapshot_hard_links_dir = get_snapshots_hardlink_dir_for_package(
        snapshot_package_output_file
            .as_ref()
            .parent()
            .expect("Invalid output path for tar"),
        slot,
    );

    let _ = fs::remove_dir_all(&snapshot_hard_links_dir);
    fs::create_dir_all(&snapshot_hard_links_dir)?;

    // Get a reference to all the relevant AccountStorageEntries
    let account_storage_entries = bank.rc.get_storage_entries();

    // Create a snapshot package
    info!(
        "Snapshot for bank: {} has {} account storage entries",
        slot,
        account_storage_entries.len()
    );
    let package = SnapshotPackage::new(
        snapshot_hard_links_dir.clone(),
        account_storage_entries,
        snapshot_package_output_file.as_ref().to_path_buf(),
    );

    // Any errors from this point on will cause the above SnapshotPackage to drop, clearing
    // any temporary state created for the SnapshotPackage (like the snapshot_hard_links_dir)
    for files in snapshot_files {
        files.hardlink_snapshot_directory(&snapshot_hard_links_dir)?;
    }

    Ok(package)
}

pub fn get_snapshot_paths<P: AsRef<Path>>(snapshot_path: P) -> Vec<SlotSnapshotPaths> {
    let paths = fs::read_dir(&snapshot_path).expect("Invalid snapshot path");
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
                snapshot_status_cache_path: snapshot_path.join(SNAPSHOT_STATUS_CACHE_FILE_NAME),
            }
        })
        .collect::<Vec<SlotSnapshotPaths>>();

    names.sort();
    names
}

pub fn add_snapshot<P: AsRef<Path>>(
    snapshot_path: P,
    bank: &Bank,
    slots_since_snapshot: &[u64],
) -> Result<()> {
    let slot = bank.slot();
    // snapshot_path/slot
    let slot_snapshot_dir = get_bank_snapshot_dir(snapshot_path, slot);
    fs::create_dir_all(slot_snapshot_dir.clone()).map_err(Error::from)?;

    // the snapshot is stored as snapshot_path/slot/slot
    let snapshot_file_path = slot_snapshot_dir.join(get_snapshot_file_name(slot));
    // the status cache is stored as snapshot_path/slot/slot_satus_cache
    let snapshot_status_cache_file_path = slot_snapshot_dir.join(SNAPSHOT_STATUS_CACHE_FILE_NAME);
    info!(
        "creating snapshot {}, path: {:?} status_cache: {:?}",
        bank.slot(),
        snapshot_file_path,
        snapshot_status_cache_file_path
    );
    let snapshot_file = File::create(&snapshot_file_path)?;
    // snapshot writer
    let mut snapshot_stream = BufWriter::new(snapshot_file);
    let status_cache = File::create(&snapshot_status_cache_file_path)?;
    // status cache writer
    let mut status_cache_stream = BufWriter::new(status_cache);

    // Create the snapshot
    serialize_into(&mut snapshot_stream, &*bank).map_err(|e| get_io_error(&e.to_string()))?;
    serialize_into(&mut snapshot_stream, &bank.rc).map_err(|e| get_io_error(&e.to_string()))?;
    // write the status cache
    serialize_into(
        &mut status_cache_stream,
        &bank.src.slot_deltas(slots_since_snapshot),
    )
    .map_err(|_| get_io_error("serialize bank status cache error"))?;

    info!(
        "successfully created snapshot {}, path: {:?} status_cache: {:?}",
        bank.slot(),
        snapshot_file_path,
        snapshot_status_cache_file_path
    );

    Ok(())
}

pub fn remove_snapshot<P: AsRef<Path>>(slot: u64, snapshot_path: P) -> Result<()> {
    let slot_snapshot_dir = get_bank_snapshot_dir(&snapshot_path, slot);
    // Remove the snapshot directory for this slot
    fs::remove_dir_all(slot_snapshot_dir)?;
    Ok(())
}

pub fn bank_from_snapshots<P>(
    local_account_paths: String,
    snapshot_paths: &[SlotSnapshotPaths],
    append_vecs_path: P,
) -> Result<Bank>
where
    P: AsRef<Path>,
{
    // Rebuild the last root bank
    let last_root_paths = snapshot_paths
        .last()
        .ok_or_else(|| get_io_error("No snapshots found in snapshots directory"))?;
    info!("Load from {:?}", &last_root_paths.snapshot_file_path);
    let file = File::open(&last_root_paths.snapshot_file_path)?;
    let mut stream = BufReader::new(file);
    let bank: Bank = deserialize_from(&mut stream).map_err(|e| get_io_error(&e.to_string()))?;

    // Rebuild accounts
    bank.rc
        .accounts_from_stream(&mut stream, local_account_paths, append_vecs_path)?;

    // merge the status caches from all previous banks
    for slot_paths in snapshot_paths.iter().rev() {
        let status_cache = File::open(&slot_paths.snapshot_status_cache_path)?;
        let mut stream = BufReader::new(status_cache);
        let slot_deltas: Vec<SlotDelta<transaction::Result<()>>> = deserialize_from(&mut stream)
            .map_err(|_| get_io_error("deserialize root error"))
            .unwrap_or_default();

        bank.src.append(&slot_deltas);
    }

    Ok(bank)
}

pub fn get_snapshot_tar_path<P: AsRef<Path>>(snapshot_output_dir: P) -> PathBuf {
    snapshot_output_dir.as_ref().join("snapshot.tgz")
}

pub fn untar_snapshot_in<P: AsRef<Path>, Q: AsRef<Path>>(
    snapshot_tar: P,
    unpack_dir: Q,
) -> Result<()> {
    let tar_gz = File::open(snapshot_tar)?;
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);
    archive.unpack(&unpack_dir)?;
    Ok(())
}

fn get_snapshot_file_name(slot: u64) -> String {
    slot.to_string()
}

fn get_bank_snapshot_dir<P: AsRef<Path>>(path: P, slot: u64) -> PathBuf {
    path.as_ref().join(slot.to_string())
}

fn get_snapshots_hardlink_dir_for_package<P: AsRef<Path>>(parent_dir: P, slot: u64) -> PathBuf {
    let file_name = format!("snapshot_{}_hard_links", slot);
    parent_dir.as_ref().join(file_name)
}

fn get_io_error(error: &str) -> Error {
    warn!("BankForks error: {:?}", error);
    Error::IO(IOError::new(ErrorKind::Other, error))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::snapshot_package::{TAR_ACCOUNTS_DIR, TAR_SNAPSHOTS_DIR};
    use tempfile::TempDir;

    pub fn verify_snapshot_tar<P, Q, R>(
        snapshot_tar: P,
        snapshots_to_verify: Q,
        storages_to_verify: R,
    ) where
        P: AsRef<Path>,
        Q: AsRef<Path>,
        R: AsRef<Path>,
    {
        let temp_dir = TempDir::new().unwrap();
        let unpack_dir = temp_dir.path();
        untar_snapshot_in(snapshot_tar, &unpack_dir).unwrap();

        // Check snapshots are the same
        let unpacked_snapshots = unpack_dir.join(&TAR_SNAPSHOTS_DIR);
        assert!(!dir_diff::is_different(&snapshots_to_verify, unpacked_snapshots).unwrap());

        // Check the account entries are the same
        let unpacked_accounts = unpack_dir.join(&TAR_ACCOUNTS_DIR);
        assert!(!dir_diff::is_different(&storages_to_verify, unpacked_accounts).unwrap());
    }
}

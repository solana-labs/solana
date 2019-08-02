use crate::result::{Error, Result};
use crate::snapshot_package::SnapshotPackage;
use bincode::{deserialize_from, serialize_into};
use flate2::read::GzDecoder;
use solana_runtime::bank::{Bank, StatusCacheRc};
use std::fs;
use std::fs::File;
use std::io::{BufReader, BufWriter, Error as IOError, ErrorKind};
use std::path::{Path, PathBuf};
use tar::Archive;

pub fn package_snapshot<P: AsRef<Path>, Q: AsRef<Path>>(
    bank: &Bank,
    snapshot_names: &[u64],
    snapshot_dir: P,
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
    trace!(
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
    for name in snapshot_names {
        hardlink_snapshot_directory(&snapshot_dir, &snapshot_hard_links_dir, *name)?;
    }

    Ok(package)
}

pub fn get_snapshot_names<P: AsRef<Path>>(snapshot_path: P) -> Vec<u64> {
    let paths = fs::read_dir(snapshot_path).expect("Invalid snapshot path");
    let mut names = paths
        .filter_map(|entry| {
            entry.ok().and_then(|e| {
                e.path()
                    .file_name()
                    .and_then(|n| n.to_str().map(|s| s.parse::<u64>().unwrap()))
            })
        })
        .collect::<Vec<u64>>();

    names.sort();
    names
}

pub fn add_snapshot<P: AsRef<Path>>(snapshot_path: P, bank: &Bank, root: u64) -> Result<()> {
    let slot = bank.slot();
    let slot_snapshot_dir = get_bank_snapshot_dir(snapshot_path, slot);
    fs::create_dir_all(slot_snapshot_dir.clone()).map_err(Error::from)?;

    let snapshot_file_path = slot_snapshot_dir.join(get_snapshot_file_name(slot));
    trace!(
        "creating snapshot {}, path: {:?}",
        bank.slot(),
        snapshot_file_path
    );
    let file = File::create(&snapshot_file_path)?;
    let mut stream = BufWriter::new(file);

    // Create the snapshot
    serialize_into(&mut stream, &*bank).map_err(|_| get_io_error("serialize bank error"))?;
    let mut parent_slot: u64 = 0;
    if let Some(parent_bank) = bank.parent() {
        parent_slot = parent_bank.slot();
    }
    serialize_into(&mut stream, &parent_slot)
        .map_err(|_| get_io_error("serialize bank parent error"))?;
    serialize_into(&mut stream, &root).map_err(|_| get_io_error("serialize root error"))?;
    serialize_into(&mut stream, &bank.src)
        .map_err(|_| get_io_error("serialize bank status cache error"))?;
    serialize_into(&mut stream, &bank.rc)
        .map_err(|_| get_io_error("serialize bank accounts error"))?;

    trace!(
        "successfully created snapshot {}, path: {:?}",
        bank.slot(),
        snapshot_file_path
    );
    Ok(())
}

pub fn remove_snapshot<P: AsRef<Path>>(slot: u64, snapshot_path: P) -> Result<()> {
    let slot_snapshot_dir = get_bank_snapshot_dir(&snapshot_path, slot);
    // Remove the snapshot directory for this slot
    fs::remove_dir_all(slot_snapshot_dir)?;
    Ok(())
}

pub fn load_snapshots<P: AsRef<Path>>(
    names: &[u64],
    bank0: &mut Bank,
    bank_maps: &mut Vec<(u64, u64, Bank)>,
    status_cache_rc: &StatusCacheRc,
    snapshot_path: P,
) -> Option<u64> {
    let mut bank_root: Option<u64> = None;

    for (i, bank_slot) in names.iter().rev().enumerate() {
        let snapshot_file_name = get_snapshot_file_name(*bank_slot);
        let snapshot_dir = get_bank_snapshot_dir(&snapshot_path, *bank_slot);
        let snapshot_file_path = snapshot_dir.join(snapshot_file_name.clone());
        trace!("Load from {:?}", snapshot_file_path);
        let file = File::open(snapshot_file_path);
        if file.is_err() {
            warn!("Snapshot file open failed for {}", bank_slot);
            continue;
        }
        let file = file.unwrap();
        let mut stream = BufReader::new(file);
        let bank: Result<Bank> =
            deserialize_from(&mut stream).map_err(|_| get_io_error("deserialize bank error"));
        let slot: Result<u64> = deserialize_from(&mut stream)
            .map_err(|_| get_io_error("deserialize bank parent error"));
        let parent_slot = if slot.is_ok() { slot.unwrap() } else { 0 };
        let root: Result<u64> =
            deserialize_from(&mut stream).map_err(|_| get_io_error("deserialize root error"));
        let status_cache: Result<StatusCacheRc> = deserialize_from(&mut stream)
            .map_err(|_| get_io_error("deserialize bank status cache error"));
        if bank_root.is_none() && bank0.rc.update_from_stream(&mut stream).is_ok() {
            bank_root = Some(root.unwrap());
        }
        if bank_root.is_some() {
            match bank {
                Ok(v) => {
                    if status_cache.is_ok() {
                        let status_cache = status_cache.unwrap();
                        status_cache_rc.append(&status_cache);
                        // On the last snapshot, purge all outdated status cache
                        // entries
                        if i == names.len() - 1 {
                            status_cache_rc.purge_roots();
                        }
                    }

                    bank_maps.push((*bank_slot, parent_slot, v));
                }
                Err(_) => warn!("Load snapshot failed for {}", bank_slot),
            }
        } else {
            warn!("Load snapshot rc failed for {}", bank_slot);
        }
    }

    bank_root
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

fn hardlink_snapshot_directory<P: AsRef<Path>, Q: AsRef<Path>>(
    snapshot_dir: P,
    snapshot_hardlink_dir: Q,
    slot: u64,
) -> Result<()> {
    // Create a new directory in snapshot_hardlink_dir
    let new_slot_hardlink_dir = snapshot_hardlink_dir.as_ref().join(slot.to_string());
    let _ = fs::remove_dir_all(&new_slot_hardlink_dir);
    fs::create_dir_all(&new_slot_hardlink_dir)?;

    // Hardlink the contents of the directory
    let snapshot_file = snapshot_dir
        .as_ref()
        .join(slot.to_string())
        .join(slot.to_string());
    fs::hard_link(
        &snapshot_file,
        &new_slot_hardlink_dir.join(slot.to_string()),
    )?;
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
    use crate::snapshot_package::{TAR_ACCOUNTS_DIR, TAR_SNAPSHOT_DIR};
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
        let unpacked_snapshots = unpack_dir.join(&TAR_SNAPSHOT_DIR);
        assert!(!dir_diff::is_different(&snapshots_to_verify, unpacked_snapshots).unwrap());

        // Check the account entries are the same
        let unpacked_accounts = unpack_dir.join(&TAR_ACCOUNTS_DIR);
        assert!(!dir_diff::is_different(&storages_to_verify, unpacked_accounts).unwrap());
    }
}

use crate::result::{Error, Result};
use crate::snapshot_package::SnapshotPackage;
use bincode::{deserialize_from, serialize_into};
use flate2::read::GzDecoder;
use solana_runtime::bank::Bank;
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
                    .and_then(|n| n.to_str().map(|s| s.parse::<u64>().ok()))
                    .unwrap_or(None)
            })
        })
        .collect::<Vec<u64>>();

    names.sort();
    names
}

pub fn add_snapshot<P: AsRef<Path>>(snapshot_path: P, bank: &Bank) -> Result<()> {
    let slot = bank.slot();
    let slot_snapshot_dir = get_bank_snapshot_dir(snapshot_path, slot);
    fs::create_dir_all(slot_snapshot_dir.clone()).map_err(Error::from)?;

    let snapshot_file_path = slot_snapshot_dir.join(get_snapshot_file_name(slot));
    info!(
        "creating snapshot {}, path: {:?}",
        bank.slot(),
        snapshot_file_path
    );
    let file = File::create(&snapshot_file_path)?;
    let mut stream = BufWriter::new(file);

    // Create the snapshot
    serialize_into(&mut stream, &*bank).map_err(|e| get_io_error(&e.to_string()))?;
    serialize_into(&mut stream, &bank.rc).map_err(|e| get_io_error(&e.to_string()))?;
    // TODO: Add status cache serialization code
    /*serialize_into(&mut stream, &bank.src).map_err(|e| get_io_error(&e.to_string()))?;*/

    info!(
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

pub fn bank_from_snapshots<P, Q>(
    local_account_paths: String,
    snapshot_path: P,
    append_vecs_path: Q,
) -> Result<Bank>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    // Rebuild the last root bank
    let names = get_snapshot_names(&snapshot_path);
    let last_root = names
        .last()
        .ok_or_else(|| get_io_error("No snapshots found in snapshots directory"))?;
    let snapshot_file_name = get_snapshot_file_name(*last_root);
    let snapshot_dir = get_bank_snapshot_dir(&snapshot_path, *last_root);
    let snapshot_file_path = snapshot_dir.join(&snapshot_file_name);
    info!("Load from {:?}", snapshot_file_path);
    let file = File::open(snapshot_file_path)?;
    let mut stream = BufReader::new(file);
    let bank: Bank = deserialize_from(&mut stream).map_err(|e| get_io_error(&e.to_string()))?;

    // Rebuild accounts
    bank.rc
        .accounts_from_stream(&mut stream, local_account_paths, append_vecs_path)?;

    for bank_slot in names.iter().rev() {
        let snapshot_file_name = get_snapshot_file_name(*bank_slot);
        let snapshot_dir = get_bank_snapshot_dir(&snapshot_path, *bank_slot);
        let snapshot_file_path = snapshot_dir.join(snapshot_file_name.clone());
        let file = File::open(snapshot_file_path)?;
        let mut stream = BufReader::new(file);
        let _bank: Result<Bank> =
            deserialize_from(&mut stream).map_err(|e| get_io_error(&e.to_string()));

        // TODO: Uncomment and deserialize status cache here

        /*let status_cache: Result<StatusCacheRc> = deserialize_from(&mut stream)
        .map_err(|e| get_io_error(&e.to_string()));
        if bank_root.is_some() {
            match bank {
                Ok(v) => {
                    if status_cache.is_ok() {
                        let status_cache = status_cache?;
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
        }*/
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

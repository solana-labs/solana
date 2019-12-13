use crate::result::{Error, Result};
use bincode::serialize_into;
use solana_ledger::snapshot_package::{SnapshotPackage, SnapshotPackageReceiver};
use solana_ledger::snapshot_utils::{self, TAR_ACCOUNTS_DIR, TAR_SNAPSHOTS_DIR};
use solana_measure::measure::Measure;
use solana_metrics::datapoint_info;
use solana_runtime::status_cache::SlotDelta;
use solana_sdk::transaction::Result as TransactionResult;
use std::fs;
use std::fs::File;
use std::io::{BufWriter, Error as IOError, ErrorKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use symlink;
use tempfile::TempDir;

pub struct SnapshotPackagerService {
    t_snapshot_packager: JoinHandle<()>,
}

impl SnapshotPackagerService {
    pub fn new(snapshot_package_receiver: SnapshotPackageReceiver, exit: &Arc<AtomicBool>) -> Self {
        let exit = exit.clone();
        let t_snapshot_packager = Builder::new()
            .name("solana-snapshot-packager".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = Self::run(&snapshot_package_receiver) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => info!("Error from package_snapshots: {:?}", e),
                    }
                }
            })
            .unwrap();
        Self {
            t_snapshot_packager,
        }
    }

    pub fn package_snapshots(snapshot_package: &SnapshotPackage) -> Result<()> {
        info!(
            "Generating snapshot tarball for root {}",
            snapshot_package.root
        );

        Self::serialize_status_cache(
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
                return Err(Self::get_io_error(
                    "Error trying to generate snapshot archive: storage path symlink is invalid",
                ));
            }
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
        ];

        let output = std::process::Command::new("tar").args(&args).output()?;
        if !output.status.success() {
            warn!("tar command failed with exit code: {}", output.status);
            use std::str::from_utf8;
            info!("tar stdout: {}", from_utf8(&output.stdout).unwrap_or("?"));
            info!("tar stderr: {}", from_utf8(&output.stderr).unwrap_or("?"));

            return Err(Self::get_io_error(&format!(
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

    fn run(snapshot_receiver: &SnapshotPackageReceiver) -> Result<()> {
        let mut snapshot_package = snapshot_receiver.recv_timeout(Duration::from_secs(1))?;
        // Only package the latest
        while let Ok(new_snapshot_package) = snapshot_receiver.try_recv() {
            snapshot_package = new_snapshot_package;
        }
        Self::package_snapshots(&snapshot_package)?;
        Ok(())
    }

    fn get_io_error(error: &str) -> Error {
        warn!("Snapshot Packaging Error: {:?}", error);
        Error::IO(IOError::new(ErrorKind::Other, error))
    }

    fn serialize_status_cache(
        slot_deltas: &[SlotDelta<TransactionResult<()>>],
        snapshot_links: &TempDir,
    ) -> Result<()> {
        // the status cache is stored as snapshot_path/status_cache
        let snapshot_status_cache_file_path = snapshot_links
            .path()
            .join(snapshot_utils::SNAPSHOT_STATUS_CACHE_FILE_NAME);

        let status_cache = File::create(&snapshot_status_cache_file_path)?;
        // status cache writer
        let mut status_cache_stream = BufWriter::new(status_cache);

        let mut status_cache_serialize = Measure::start("status_cache_serialize-ms");
        // write the status cache
        serialize_into(&mut status_cache_stream, slot_deltas)
            .map_err(|_| Self::get_io_error("serialize status cache error"))?;
        status_cache_serialize.stop();
        inc_new_counter_info!(
            "serialize-status-cache-ms",
            status_cache_serialize.as_ms() as usize
        );
        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_snapshot_packager.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_ledger::snapshot_utils;
    use solana_runtime::accounts_db::AccountStorageEntry;
    use std::{
        fs::{remove_dir_all, OpenOptions},
        io::Write,
        path::{Path, PathBuf},
    };
    use tempfile::TempDir;

    // Create temporary placeholder directory for all test files
    fn make_tmp_dir_path() -> PathBuf {
        let out_dir = std::env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
        let path = PathBuf::from(format!("{}/tmp/test_package_snapshots", out_dir));

        // whack any possible collision
        let _ignored = std::fs::remove_dir_all(&path);
        // whack any possible collision
        let _ignored = std::fs::remove_file(&path);

        path
    }

    #[test]
    fn test_package_snapshots_relative_ledger_path() {
        let temp_dir = make_tmp_dir_path();
        create_and_verify_snapshot(&temp_dir);
        remove_dir_all(temp_dir).expect("should remove tmp dir");
    }

    #[test]
    fn test_package_snapshots() {
        create_and_verify_snapshot(TempDir::new().unwrap().path())
    }

    fn create_and_verify_snapshot(temp_dir: &Path) {
        let accounts_dir = temp_dir.join("accounts");
        let snapshots_dir = temp_dir.join("snapshots");
        let snapshot_package_output_path = temp_dir.join("snapshots_output");
        fs::create_dir_all(&snapshot_package_output_path).unwrap();

        fs::create_dir_all(&accounts_dir).unwrap();
        // Create some storage entries
        let storage_entries: Vec<_> = (0..5)
            .map(|i| Arc::new(AccountStorageEntry::new(&accounts_dir, 0, i, 10)))
            .collect();

        // Create some fake snapshot
        fs::create_dir_all(&snapshots_dir).unwrap();
        let snapshots_paths: Vec<_> = (0..5)
            .map(|i| {
                let fake_snapshot_path = snapshots_dir.join(format!("fake_snapshot_{}", i));
                let mut fake_snapshot_file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(&fake_snapshot_path)
                    .unwrap();

                fake_snapshot_file.write_all(b"Hello, world!").unwrap();
                fake_snapshot_path
            })
            .collect();

        // Create directory of hard links for snapshots
        let link_snapshots_dir = tempfile::tempdir_in(&temp_dir).unwrap();
        for snapshots_path in snapshots_paths {
            let snapshot_file_name = snapshots_path.file_name().unwrap();
            let link_path = link_snapshots_dir.path().join(snapshot_file_name);
            fs::hard_link(&snapshots_path, &link_path).unwrap();
        }

        // Create a packageable snapshot
        let output_tar_path = snapshot_utils::get_snapshot_tar_path(&snapshot_package_output_path);
        let snapshot_package = SnapshotPackage::new(
            5,
            vec![],
            link_snapshots_dir,
            storage_entries.clone(),
            output_tar_path.clone(),
        );

        // Make tarball from packageable snapshot
        SnapshotPackagerService::package_snapshots(&snapshot_package).unwrap();

        // before we compare, stick an empty status_cache in this dir so that the package comparision works
        // This is needed since the status_cache is added by the packager and is not collected from
        // the source dir for snapshots
        let slot_deltas: Vec<SlotDelta<TransactionResult<()>>> = vec![];
        let dummy_status_cache = File::create(snapshots_dir.join("status_cache")).unwrap();
        let mut status_cache_stream = BufWriter::new(dummy_status_cache);
        serialize_into(&mut status_cache_stream, &slot_deltas).unwrap();
        status_cache_stream.flush().unwrap();

        // Check tarball is correct
        snapshot_utils::verify_snapshot_tar(output_tar_path, snapshots_dir, accounts_dir);
    }
}

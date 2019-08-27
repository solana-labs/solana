use crate::result::{Error, Result};
use crate::service::Service;
use solana_measure::measure::Measure;
use solana_metrics::datapoint_info;
use solana_runtime::accounts_db::AccountStorageEntry;
use std::fs;
use std::io::{Error as IOError, ErrorKind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use symlink;
use tempfile::TempDir;

pub type SnapshotPackageSender = Sender<SnapshotPackage>;
pub type SnapshotPackageReceiver = Receiver<SnapshotPackage>;

pub const TAR_SNAPSHOTS_DIR: &str = "snapshots";
pub const TAR_ACCOUNTS_DIR: &str = "accounts";

pub struct SnapshotPackage {
    root: u64,
    snapshot_links: TempDir,
    storage_entries: Vec<Arc<AccountStorageEntry>>,
    tar_output_file: PathBuf,
}

impl SnapshotPackage {
    pub fn new(
        root: u64,
        snapshot_links: TempDir,
        storage_entries: Vec<Arc<AccountStorageEntry>>,
        tar_output_file: PathBuf,
    ) -> Self {
        Self {
            root,
            snapshot_links,
            storage_entries,
            tar_output_file,
        }
    }
}

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
            let storage_path = storage.get_path();
            let output_path = staging_accounts_dir.join(
                storage_path
                    .file_name()
                    .expect("Invalid AppendVec file path"),
            );

            // `storage_path` - The file path where the AppendVec itself is located
            // `output_path` - The directory where the AppendVec will be placed in the staging directory.
            symlink::symlink_dir(storage_path, output_path)?;
        }

        // Tar the staging directory into the archive `temp_tar_gz`
        let temp_tar_gz = tempfile::Builder::new()
            .prefix("new_state")
            .suffix(".tar.bz2")
            .tempfile_in(tar_dir)?;
        let temp_tar_path = temp_tar_gz.path();
        let mut args = vec!["jcfhS"];
        args.push(temp_tar_path.to_str().unwrap());
        args.push("-C");
        args.push(staging_dir.path().to_str().unwrap());
        args.push(TAR_ACCOUNTS_DIR);
        args.push(TAR_SNAPSHOTS_DIR);

        let status = std::process::Command::new("tar").args(&args).status()?;

        if !status.success() {
            return Err(Self::get_io_error(&format!(
                "Error trying to generate snapshot archive: {}",
                status
            )));
        }

        // Once everything is successful, overwrite the previous tarball so that other validators
        // can fetch this newly packaged snapshot
        let _ = fs::remove_file(&snapshot_package.tar_output_file);
        let metadata = fs::metadata(&temp_tar_path)?;
        fs::hard_link(&temp_tar_path, &snapshot_package.tar_output_file)?;

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
}

impl Service for SnapshotPackagerService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_snapshot_packager.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot_utils;
    use std::fs::OpenOptions;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_package_snapshots() {
        // Create temporary placeholder directory for all test files
        let temp_dir = TempDir::new().unwrap();
        let accounts_dir = temp_dir.path().join("accounts");
        let snapshots_dir = temp_dir.path().join("snapshots");
        let snapshot_package_output_path = temp_dir.path().join("snapshots_output");
        fs::create_dir_all(&snapshot_package_output_path).unwrap();

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
        let link_snapshots_dir = tempfile::tempdir_in(temp_dir.path()).unwrap();
        for snapshots_path in snapshots_paths {
            let snapshot_file_name = snapshots_path.file_name().unwrap();
            let link_path = link_snapshots_dir.path().join(snapshot_file_name);
            fs::hard_link(&snapshots_path, &link_path).unwrap();
        }

        // Create a packageable snapshot
        let output_tar_path = snapshot_utils::get_snapshot_tar_path(&snapshot_package_output_path);
        let snapshot_package = SnapshotPackage::new(
            5,
            link_snapshots_dir,
            storage_entries.clone(),
            output_tar_path.clone(),
        );

        // Make tarball from packageable snapshot
        SnapshotPackagerService::package_snapshots(&snapshot_package).unwrap();

        // Check tarball is correct
        snapshot_utils::tests::verify_snapshot_tar(output_tar_path, snapshots_dir, accounts_dir);
    }
}

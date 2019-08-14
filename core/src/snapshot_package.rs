use crate::result::{Error, Result};
use crate::service::Service;
use flate2::write::GzEncoder;
use flate2::Compression;
use solana_runtime::accounts_db::AccountStorageEntry;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};
use std::time::Duration;
use tempfile::TempDir;

pub type SnapshotPackageSender = Sender<SnapshotPackage>;
pub type SnapshotPackageReceiver = Receiver<SnapshotPackage>;

pub const TAR_SNAPSHOTS_DIR: &str = "snapshots";
pub const TAR_ACCOUNTS_DIR: &str = "accounts";

pub struct SnapshotPackage {
    snapshot_links: TempDir,
    storage_entries: Vec<Arc<AccountStorageEntry>>,
    tar_output_file: PathBuf,
}

impl SnapshotPackage {
    pub fn new(
        snapshot_links: TempDir,
        storage_entries: Vec<Arc<AccountStorageEntry>>,
        tar_output_file: PathBuf,
    ) -> Self {
        Self {
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
        let tar_dir = snapshot_package
            .tar_output_file
            .parent()
            .expect("Tar output path is invalid");

        fs::create_dir_all(tar_dir)?;

        // Create the tar builder
        let tar_gz = tempfile::Builder::new()
            .prefix("new_state")
            .suffix(".tgz")
            .tempfile_in(tar_dir)?;

        let temp_tar_path = tar_gz.path();
        let enc = GzEncoder::new(&tar_gz, Compression::default());
        let mut tar = tar::Builder::new(enc);

        // Create the list of paths to compress, starting with the snapshots
        let tar_output_snapshots_dir = Path::new(&TAR_SNAPSHOTS_DIR);

        // Add the snapshots to the tarball and delete the directory of hardlinks to the snapshots
        // that was created to persist those snapshots while this package was being created
        let res = tar.append_dir_all(
            tar_output_snapshots_dir,
            snapshot_package.snapshot_links.path(),
        );
        res?;

        // Add the AppendVecs into the compressible list
        let tar_output_accounts_dir = Path::new(&TAR_ACCOUNTS_DIR);
        for storage in &snapshot_package.storage_entries {
            let storage_path = storage.get_path();
            let output_path = tar_output_accounts_dir.join(
                storage_path
                    .file_name()
                    .expect("Invalid AppendVec file path"),
            );

            // `output_path` - The directory where the AppendVec will be placed in the tarball.
            // `storage_path` - The file path where the AppendVec itself is located
            tar.append_path_with_name(storage_path, output_path)?;
        }

        // Once everything is successful, overwrite the previous tarball so that other validators
        // can rsync this newly packaged snapshot
        tar.finish()?;
        let _ = fs::remove_file(&snapshot_package.tar_output_file);
        fs::hard_link(&temp_tar_path, &snapshot_package.tar_output_file)?;
        Ok(())
    }

    fn run(snapshot_receiver: &SnapshotPackageReceiver) -> Result<()> {
        let mut snapshot_package = snapshot_receiver.recv_timeout(Duration::from_secs(1))?;
        // Only package the latest
        while let Ok(new_snapshot_package) = snapshot_receiver.recv() {
            snapshot_package = new_snapshot_package;
        }
        Self::package_snapshots(&snapshot_package)?;
        Ok(())
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
        // Create temprorary placeholder directory for all test files
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

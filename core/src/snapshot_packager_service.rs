use {
    solana_gossip::cluster_info::{
        ClusterInfo, MAX_INCREMENTAL_SNAPSHOT_HASHES, MAX_SNAPSHOT_HASHES,
    },
    solana_perf::thread::renice_this_thread,
    solana_runtime::{
        snapshot_archive_info::SnapshotArchiveInfoGetter,
        snapshot_config::SnapshotConfig,
        snapshot_hash::{
            FullSnapshotHash, FullSnapshotHashes, IncrementalSnapshotHash,
            IncrementalSnapshotHashes, StartingSnapshotHashes,
        },
        snapshot_package::{retain_max_n_elements, PendingSnapshotPackage, SnapshotType},
        snapshot_utils,
    },
    solana_sdk::{clock::Slot, hash::Hash},
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

pub struct SnapshotPackagerService {
    t_snapshot_packager: JoinHandle<()>,
}

impl SnapshotPackagerService {
    pub fn new(
        pending_snapshot_package: PendingSnapshotPackage,
        starting_snapshot_hashes: Option<StartingSnapshotHashes>,
        exit: &Arc<AtomicBool>,
        cluster_info: &Arc<ClusterInfo>,
        snapshot_config: SnapshotConfig,
        enable_gossip_push: bool,
    ) -> Self {
        let exit = exit.clone();
        let cluster_info = cluster_info.clone();
        let max_full_snapshot_hashes = std::cmp::min(
            MAX_SNAPSHOT_HASHES,
            snapshot_config.maximum_full_snapshot_archives_to_retain,
        );
        let max_incremental_snapshot_hashes = std::cmp::min(
            MAX_INCREMENTAL_SNAPSHOT_HASHES,
            snapshot_config.maximum_incremental_snapshot_archives_to_retain,
        );

        let t_snapshot_packager = Builder::new()
            .name("solSnapshotPkgr".to_string())
            .spawn(move || {
                renice_this_thread(snapshot_config.packager_thread_niceness_adj).unwrap();
                let mut snapshot_gossip_manager = if enable_gossip_push {
                    Some(SnapshotGossipManager {
                        cluster_info,
                        max_full_snapshot_hashes,
                        max_incremental_snapshot_hashes,
                        full_snapshot_hashes: FullSnapshotHashes::default(),
                        incremental_snapshot_hashes: IncrementalSnapshotHashes::default(),
                    })
                } else {
                    None
                };
                if let Some(snapshot_gossip_manager) = snapshot_gossip_manager.as_mut() {
                    snapshot_gossip_manager.push_starting_snapshot_hashes(starting_snapshot_hashes);
                }

                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    let snapshot_package = pending_snapshot_package.lock().unwrap().take();
                    if snapshot_package.is_none() {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    let snapshot_package = snapshot_package.unwrap();

                    // Archiving the snapshot package is not allowed to fail.
                    // AccountsBackgroundService calls `clean_accounts()` with a value for
                    // last_full_snapshot_slot that requires this archive call to succeed.
                    snapshot_utils::archive_snapshot_package(
                        &snapshot_package,
                        &snapshot_config.full_snapshot_archives_dir,
                        &snapshot_config.incremental_snapshot_archives_dir,
                        snapshot_config.maximum_full_snapshot_archives_to_retain,
                        snapshot_config.maximum_incremental_snapshot_archives_to_retain,
                    )
                    .expect("failed to archive snapshot package");

                    if let Some(snapshot_gossip_manager) = snapshot_gossip_manager.as_mut() {
                        snapshot_gossip_manager.push_snapshot_hash(
                            snapshot_package.snapshot_type,
                            (snapshot_package.slot(), snapshot_package.hash().0),
                        );
                    }
                }
            })
            .unwrap();

        Self {
            t_snapshot_packager,
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_snapshot_packager.join()
    }
}

struct SnapshotGossipManager {
    cluster_info: Arc<ClusterInfo>,
    max_full_snapshot_hashes: usize,
    max_incremental_snapshot_hashes: usize,
    full_snapshot_hashes: FullSnapshotHashes,
    incremental_snapshot_hashes: IncrementalSnapshotHashes,
}

impl SnapshotGossipManager {
    /// If there were starting snapshot hashes, add those to their respective vectors, then push
    /// those vectors to the cluster via CRDS.
    fn push_starting_snapshot_hashes(
        &mut self,
        starting_snapshot_hashes: Option<StartingSnapshotHashes>,
    ) {
        if let Some(starting_snapshot_hashes) = starting_snapshot_hashes {
            let starting_full_snapshot_hash = starting_snapshot_hashes.full;
            self.push_full_snapshot_hash(starting_full_snapshot_hash);

            if let Some(starting_incremental_snapshot_hash) = starting_snapshot_hashes.incremental {
                self.push_incremental_snapshot_hash(starting_incremental_snapshot_hash);
            };
        }
    }

    /// Add `snapshot_hash` to its respective vector of hashes, then push that vector to the
    /// cluster via CRDS.
    fn push_snapshot_hash(&mut self, snapshot_type: SnapshotType, snapshot_hash: (Slot, Hash)) {
        match snapshot_type {
            SnapshotType::FullSnapshot => {
                self.push_full_snapshot_hash(FullSnapshotHash {
                    hash: snapshot_hash,
                });
            }
            SnapshotType::IncrementalSnapshot(base_slot) => {
                let latest_full_snapshot_hash = *self.full_snapshot_hashes.hashes.last().unwrap();
                assert_eq!(
                    base_slot, latest_full_snapshot_hash.0,
                    "the incremental snapshot's base slot ({}) must match the latest full snapshot hash's slot ({})",
                    base_slot, latest_full_snapshot_hash.0,
                );
                self.push_incremental_snapshot_hash(IncrementalSnapshotHash {
                    base: latest_full_snapshot_hash,
                    hash: snapshot_hash,
                });
            }
        }
    }

    /// Add `full_snapshot_hash` to the vector of full snapshot hashes, then push that vector to
    /// the cluster via CRDS.
    fn push_full_snapshot_hash(&mut self, full_snapshot_hash: FullSnapshotHash) {
        self.full_snapshot_hashes
            .hashes
            .push(full_snapshot_hash.hash);

        retain_max_n_elements(
            &mut self.full_snapshot_hashes.hashes,
            self.max_full_snapshot_hashes,
        );

        self.cluster_info
            .push_snapshot_hashes(self.full_snapshot_hashes.hashes.clone());
    }

    /// Add `incremental_snapshot_hash` to the vector of incremental snapshot hashes, then push
    /// that vector to the cluster via CRDS.
    fn push_incremental_snapshot_hash(
        &mut self,
        incremental_snapshot_hash: IncrementalSnapshotHash,
    ) {
        // If the base snapshot hash is different from the one in IncrementalSnapshotHashes, then
        // that means the old incremental snapshot hashes are no longer valid, so clear them all
        // out.
        if incremental_snapshot_hash.base != self.incremental_snapshot_hashes.base {
            self.incremental_snapshot_hashes.hashes.clear();
            self.incremental_snapshot_hashes.base = incremental_snapshot_hash.base;
        }

        self.incremental_snapshot_hashes
            .hashes
            .push(incremental_snapshot_hash.hash);

        retain_max_n_elements(
            &mut self.incremental_snapshot_hashes.hashes,
            self.max_incremental_snapshot_hashes,
        );

        // Pushing incremental snapshot hashes to the cluster should never fail.  The only error
        // case is when the length of the hashes is too big, but we account for that with
        // `max_incremental_snapshot_hashes`.  If this call ever does error, it's a programmer bug!
        // Check to see what changed in `push_incremental_snapshot_hashes()` and handle the new
        // error condition here.
        self.cluster_info
            .push_incremental_snapshot_hashes(
                self.incremental_snapshot_hashes.base,
                self.incremental_snapshot_hashes.hashes.clone(),
            )
            .expect(
                "Bug! The programmer contract has changed for push_incremental_snapshot_hashes() \
                 and a new error case has been added, which has not been handled here.",
            );
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        bincode::serialize_into,
        solana_runtime::{
            accounts_db::AccountStorageEntry,
            bank::BankSlotDelta,
            snapshot_archive_info::SnapshotArchiveInfo,
            snapshot_hash::SnapshotHash,
            snapshot_package::{SnapshotPackage, SnapshotType},
            snapshot_utils::{
                self, ArchiveFormat, SnapshotVersion, SNAPSHOT_STATUS_CACHE_FILENAME,
            },
        },
        solana_sdk::hash::Hash,
        std::{
            fs::{self, remove_dir_all, OpenOptions},
            io::Write,
            path::{Path, PathBuf},
        },
        tempfile::TempDir,
    };

    // Create temporary placeholder directory for all test files
    fn make_tmp_dir_path() -> PathBuf {
        let out_dir = std::env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
        let path = PathBuf::from(format!("{out_dir}/tmp/test_package_snapshots"));

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
        let full_snapshot_archives_dir = temp_dir.join("full_snapshot_archives");
        let incremental_snapshot_archives_dir = temp_dir.join("incremental_snapshot_archives");
        fs::create_dir_all(&full_snapshot_archives_dir).unwrap();
        fs::create_dir_all(&incremental_snapshot_archives_dir).unwrap();

        fs::create_dir_all(&accounts_dir).unwrap();
        // Create some storage entries
        let storage_entries: Vec<_> = (0..5)
            .map(|i| Arc::new(AccountStorageEntry::new(&accounts_dir, 0, i, 10)))
            .collect();

        // Create some fake snapshot
        let snapshots_paths: Vec<_> = (0..5)
            .map(|i| {
                let snapshot_file_name = format!("{i}");
                let snapshots_dir = snapshots_dir.join(&snapshot_file_name);
                fs::create_dir_all(&snapshots_dir).unwrap();
                let fake_snapshot_path = snapshots_dir.join(&snapshot_file_name);
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
        let link_snapshots_dir = tempfile::tempdir_in(temp_dir).unwrap();
        for snapshots_path in snapshots_paths {
            let snapshot_file_name = snapshots_path.file_name().unwrap();
            let link_snapshots_dir = link_snapshots_dir.path().join(snapshot_file_name);
            fs::create_dir_all(&link_snapshots_dir).unwrap();
            let link_path = link_snapshots_dir.join(snapshot_file_name);
            fs::hard_link(&snapshots_path, link_path).unwrap();
        }

        // Create a packageable snapshot
        let slot = 42;
        let hash = SnapshotHash(Hash::default());
        let archive_format = ArchiveFormat::TarBzip2;
        let output_tar_path = snapshot_utils::build_full_snapshot_archive_path(
            &full_snapshot_archives_dir,
            slot,
            &hash,
            archive_format,
        );
        let snapshot_package = SnapshotPackage {
            snapshot_archive_info: SnapshotArchiveInfo {
                path: output_tar_path.clone(),
                slot,
                hash,
                archive_format,
            },
            block_height: slot,
            slot_deltas: vec![],
            snapshot_links: link_snapshots_dir,
            snapshot_storages: vec![storage_entries],
            snapshot_version: SnapshotVersion::default(),
            snapshot_type: SnapshotType::FullSnapshot,
        };

        // Make tarball from packageable snapshot
        snapshot_utils::archive_snapshot_package(
            &snapshot_package,
            full_snapshot_archives_dir,
            incremental_snapshot_archives_dir,
            snapshot_utils::DEFAULT_MAX_FULL_SNAPSHOT_ARCHIVES_TO_RETAIN,
            snapshot_utils::DEFAULT_MAX_INCREMENTAL_SNAPSHOT_ARCHIVES_TO_RETAIN,
        )
        .unwrap();

        // before we compare, stick an empty status_cache in this dir so that the package comparison works
        // This is needed since the status_cache is added by the packager and is not collected from
        // the source dir for snapshots
        let dummy_slot_deltas: Vec<BankSlotDelta> = vec![];
        snapshot_utils::serialize_snapshot_data_file(
            &snapshots_dir.join(SNAPSHOT_STATUS_CACHE_FILENAME),
            |stream| {
                serialize_into(stream, &dummy_slot_deltas)?;
                Ok(())
            },
        )
        .unwrap();

        // Check archive is correct
        snapshot_utils::verify_snapshot_archive(
            output_tar_path,
            snapshots_dir,
            accounts_dir,
            archive_format,
            snapshot_utils::VerifyBank::Deterministic,
        );
    }
}

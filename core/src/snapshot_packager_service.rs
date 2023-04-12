mod snapshot_gossip_manager;
use {
    crossbeam_channel::{Receiver, Sender},
    snapshot_gossip_manager::SnapshotGossipManager,
    solana_gossip::cluster_info::{
        ClusterInfo, MAX_INCREMENTAL_SNAPSHOT_HASHES, MAX_LEGACY_SNAPSHOT_HASHES,
    },
    solana_measure::measure_us,
    solana_perf::thread::renice_this_thread,
    solana_runtime::{
        snapshot_archive_info::SnapshotArchiveInfoGetter,
        snapshot_config::SnapshotConfig,
        snapshot_hash::StartingSnapshotHashes,
        snapshot_package::{self, SnapshotPackage},
        snapshot_utils,
    },
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
    /// If there are no snapshot packages to handle, limit how often we re-check
    const LOOP_LIMITER: Duration = Duration::from_millis(100);

    pub fn new(
        snapshot_package_sender: Sender<SnapshotPackage>,
        snapshot_package_receiver: Receiver<SnapshotPackage>,
        starting_snapshot_hashes: Option<StartingSnapshotHashes>,
        exit: &Arc<AtomicBool>,
        cluster_info: &Arc<ClusterInfo>,
        snapshot_config: SnapshotConfig,
        enable_gossip_push: bool,
    ) -> Self {
        let exit = exit.clone();
        let cluster_info = cluster_info.clone();
        let max_full_snapshot_hashes = std::cmp::min(
            MAX_LEGACY_SNAPSHOT_HASHES,
            snapshot_config
                .maximum_full_snapshot_archives_to_retain
                .get(),
        );
        let max_incremental_snapshot_hashes = std::cmp::min(
            MAX_INCREMENTAL_SNAPSHOT_HASHES,
            snapshot_config
                .maximum_incremental_snapshot_archives_to_retain
                .get(),
        );

        let t_snapshot_packager = Builder::new()
            .name("solSnapshotPkgr".to_string())
            .spawn(move || {
                info!("SnapshotPackagerService has started");
                renice_this_thread(snapshot_config.packager_thread_niceness_adj).unwrap();
                let mut snapshot_gossip_manager = enable_gossip_push.then(||
                    SnapshotGossipManager::new(
                        cluster_info,
                        max_full_snapshot_hashes,
                        max_incremental_snapshot_hashes,
                    )
                );
                if let Some(snapshot_gossip_manager) = snapshot_gossip_manager.as_mut() {
                    snapshot_gossip_manager.push_starting_snapshot_hashes(starting_snapshot_hashes);
                }

                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }

                    let Some((
                        snapshot_package,
                        num_outstanding_snapshot_packages,
                        num_re_enqueued_snapshot_packages,
                    )) = Self::get_next_snapshot_package(&snapshot_package_sender, &snapshot_package_receiver) else {
                        std::thread::sleep(Self::LOOP_LIMITER);
                        continue;
                    };
                    info!("handling snapshot package: {snapshot_package:?}");
                    let enqueued_time = snapshot_package.enqueued.elapsed();

                    let (_, handling_time_us) = measure_us!({
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
                                (snapshot_package.slot(), *snapshot_package.hash()),
                            );
                        }
                    });

                    datapoint_info!(
                        "snapshot_packager_service",
                        (
                            "num-outstanding-snapshot-packages",
                            num_outstanding_snapshot_packages,
                            i64
                        ),
                        (
                            "num-re-enqueued-snapshot-packages",
                            num_re_enqueued_snapshot_packages,
                            i64
                        ),
                        ("enqueued-time-us", enqueued_time.as_micros(), i64),
                        ("handling-time-us", handling_time_us, i64),
                    );
                }
                info!("SnapshotPackagerService has stopped");
            })
            .unwrap();

        Self {
            t_snapshot_packager,
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_snapshot_packager.join()
    }

    /// Get the next snapshot package to handle
    ///
    /// Look through the snapshot package channel to find the highest priority one to handle next.
    /// If there are no snapshot packages in the channel, return None.  Otherwise return the
    /// highest priority one.  Unhandled snapshot packages with slots GREATER-THAN the handled one
    /// will be re-enqueued.  The remaining will be dropped.
    ///
    /// Also return the number of snapshot packages initially in the channel, and the number of
    /// ones re-enqueued.
    fn get_next_snapshot_package(
        snapshot_package_sender: &Sender<SnapshotPackage>,
        snapshot_package_receiver: &Receiver<SnapshotPackage>,
    ) -> Option<(
        SnapshotPackage,
        /*num outstanding snapshot packages*/ usize,
        /*num re-enqueued snapshot packages*/ usize,
    )> {
        let mut snapshot_packages: Vec<_> = snapshot_package_receiver.try_iter().collect();
        // `select_nth()` panics if the slice is empty, so return if that's the case
        if snapshot_packages.is_empty() {
            return None;
        }
        let snapshot_packages_len = snapshot_packages.len();
        debug!("outstanding snapshot packages ({snapshot_packages_len}): {snapshot_packages:?}");

        snapshot_packages.select_nth_unstable_by(
            snapshot_packages_len - 1,
            snapshot_package::cmp_snapshot_packages_by_priority,
        );
        // SAFETY: We know `snapshot_packages` is not empty, so its len is >= 1,
        // therefore there is always an element to pop.
        let snapshot_package = snapshot_packages.pop().unwrap();
        let handled_snapshot_package_slot = snapshot_package.slot();
        // re-enqueue any remaining snapshot packages for slots GREATER-THAN the snapshot package
        // that will be handled
        let num_re_enqueued_snapshot_packages = snapshot_packages
            .into_iter()
            .filter(|snapshot_package| snapshot_package.slot() > handled_snapshot_package_slot)
            .map(|snapshot_package| {
                snapshot_package_sender
                    .try_send(snapshot_package)
                    .expect("re-enqueue snapshot package")
            })
            .count();

        Some((
            snapshot_package,
            snapshot_packages_len,
            num_re_enqueued_snapshot_packages,
        ))
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        bincode::serialize_into,
        rand::seq::SliceRandom,
        solana_runtime::{
            accounts_db::AccountStorageEntry,
            bank::BankSlotDelta,
            snapshot_archive_info::SnapshotArchiveInfo,
            snapshot_hash::SnapshotHash,
            snapshot_package::{SnapshotPackage, SnapshotType},
            snapshot_utils::{
                self, create_accounts_run_and_snapshot_dirs, ArchiveFormat, SnapshotVersion,
                SNAPSHOT_STATUS_CACHE_FILENAME,
            },
        },
        solana_sdk::{clock::Slot, hash::Hash},
        std::{
            fs::{self, remove_dir_all, OpenOptions},
            io::Write,
            path::{Path, PathBuf},
            time::Instant,
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
        let accounts_dir = create_accounts_run_and_snapshot_dirs(accounts_dir)
            .unwrap()
            .0;

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
            snapshot_links: link_snapshots_dir,
            snapshot_storages: storage_entries,
            snapshot_version: SnapshotVersion::default(),
            snapshot_type: SnapshotType::FullSnapshot,
            enqueued: Instant::now(),
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

    /// Ensure that unhandled snapshot packages are properly re-enqueued or dropped
    ///
    /// The snapshot package handler should re-enqueue unhandled snapshot packages, if those
    /// unhandled snapshot packages are for slots GREATER-THAN the last handled snapshot package.
    /// Otherwise, they should be dropped.
    #[test]
    fn test_get_next_snapshot_package() {
        fn new(snapshot_type: SnapshotType, slot: Slot) -> SnapshotPackage {
            SnapshotPackage {
                snapshot_archive_info: SnapshotArchiveInfo {
                    path: PathBuf::default(),
                    slot,
                    hash: SnapshotHash(Hash::default()),
                    archive_format: ArchiveFormat::Tar,
                },
                block_height: slot,
                snapshot_links: TempDir::new().unwrap(),
                snapshot_storages: Vec::default(),
                snapshot_version: SnapshotVersion::default(),
                snapshot_type,
                enqueued: Instant::now(),
            }
        }
        fn new_full(slot: Slot) -> SnapshotPackage {
            new(SnapshotType::FullSnapshot, slot)
        }
        fn new_incr(slot: Slot, base: Slot) -> SnapshotPackage {
            new(SnapshotType::IncrementalSnapshot(base), slot)
        }

        let (snapshot_package_sender, snapshot_package_receiver) = crossbeam_channel::unbounded();

        // Populate the channel so that re-enqueueing and dropping will be tested
        let mut snapshot_packages = [
            new_full(100),
            new_incr(110, 100),
            new_incr(210, 100),
            new_full(300),
            new_incr(310, 300),
            new_full(400), // <-- handle 1st
            new_incr(410, 400),
            new_incr(420, 400), // <-- handle 2nd
        ];
        // Shuffle the snapshot packages to simulate receiving new snapshot packages from AHV
        // simultaneously as SPS is handling them.
        snapshot_packages.shuffle(&mut rand::thread_rng());
        snapshot_packages
            .into_iter()
            .for_each(|snapshot_package| snapshot_package_sender.send(snapshot_package).unwrap());

        // The Full Snapshot from slot 400 is handled 1st
        // (the older full snapshots are skipped and dropped)
        let (
            snapshot_package,
            _num_outstanding_snapshot_packages,
            num_re_enqueued_snapshot_packages,
        ) = SnapshotPackagerService::get_next_snapshot_package(
            &snapshot_package_sender,
            &snapshot_package_receiver,
        )
        .unwrap();
        assert_eq!(snapshot_package.snapshot_type, SnapshotType::FullSnapshot,);
        assert_eq!(snapshot_package.slot(), 400);
        assert_eq!(num_re_enqueued_snapshot_packages, 2);

        // The Incremental Snapshot from slot 420 is handled 2nd
        // (the older incremental snapshot from slot 410 is skipped and dropped)
        let (
            snapshot_package,
            _num_outstanding_snapshot_packages,
            num_re_enqueued_snapshot_packages,
        ) = SnapshotPackagerService::get_next_snapshot_package(
            &snapshot_package_sender,
            &snapshot_package_receiver,
        )
        .unwrap();
        assert_eq!(
            snapshot_package.snapshot_type,
            SnapshotType::IncrementalSnapshot(400),
        );
        assert_eq!(snapshot_package.slot(), 420);
        assert_eq!(num_re_enqueued_snapshot_packages, 0);

        // And now the snapshot package channel is empty!
        assert!(SnapshotPackagerService::get_next_snapshot_package(
            &snapshot_package_sender,
            &snapshot_package_receiver
        )
        .is_none());
    }
}

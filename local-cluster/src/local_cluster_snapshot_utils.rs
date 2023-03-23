use {
    crate::{cluster::Cluster, local_cluster::LocalCluster},
    log::*,
    solana_runtime::{
        snapshot_archive_info::{
            FullSnapshotArchiveInfo, IncrementalSnapshotArchiveInfo, SnapshotArchiveInfoGetter,
        },
        snapshot_utils,
    },
    solana_sdk::{client::SyncClient, commitment_config::CommitmentConfig},
    std::{
        path::Path,
        thread::sleep,
        time::{Duration, Instant},
    },
};

impl LocalCluster {
    /// Return the next full snapshot archive info after the cluster's last processed slot
    pub fn wait_for_next_full_snapshot<T>(
        &self,
        full_snapshot_archives_dir: T,
        max_wait_duration: Option<Duration>,
    ) -> FullSnapshotArchiveInfo
    where
        T: AsRef<Path>,
    {
        match self.wait_for_next_snapshot(
            full_snapshot_archives_dir,
            None::<T>,
            NextSnapshotType::FullSnapshot,
            max_wait_duration,
        ) {
            NextSnapshotResult::FullSnapshot(full_snapshot_archive_info) => {
                full_snapshot_archive_info
            }
            _ => unreachable!(),
        }
    }

    /// Return the next incremental snapshot archive info (and associated full snapshot archive info)
    /// after the cluster's last processed slot
    pub fn wait_for_next_incremental_snapshot(
        &self,
        full_snapshot_archives_dir: impl AsRef<Path>,
        incremental_snapshot_archives_dir: impl AsRef<Path>,
        max_wait_duration: Option<Duration>,
    ) -> (IncrementalSnapshotArchiveInfo, FullSnapshotArchiveInfo) {
        match self.wait_for_next_snapshot(
            full_snapshot_archives_dir,
            Some(incremental_snapshot_archives_dir),
            NextSnapshotType::IncrementalAndFullSnapshot,
            max_wait_duration,
        ) {
            NextSnapshotResult::IncrementalAndFullSnapshot(
                incremental_snapshot_archive_info,
                full_snapshot_archive_info,
            ) => (
                incremental_snapshot_archive_info,
                full_snapshot_archive_info,
            ),
            _ => unreachable!(),
        }
    }

    /// Return the next snapshot archive infos after the cluster's last processed slot
    fn wait_for_next_snapshot(
        &self,
        full_snapshot_archives_dir: impl AsRef<Path>,
        incremental_snapshot_archives_dir: Option<impl AsRef<Path>>,
        next_snapshot_type: NextSnapshotType,
        max_wait_duration: Option<Duration>,
    ) -> NextSnapshotResult {
        // Get slot after which this was generated
        let client = self
            .get_validator_client(self.entry_point_info.pubkey())
            .unwrap();
        let last_slot = client
            .get_slot_with_commitment(CommitmentConfig::processed())
            .expect("Couldn't get slot");

        // Wait for a snapshot for a bank >= last_slot to be made so we know that the snapshot
        // must include the transactions just pushed
        trace!(
            "Waiting for {:?} snapshot archive to be generated with slot >= {}, max wait duration: {:?}",
            next_snapshot_type,
            last_slot,
            max_wait_duration,
        );
        let timer = Instant::now();
        let next_snapshot = loop {
            if let Some(full_snapshot_archive_info) =
                snapshot_utils::get_highest_full_snapshot_archive_info(&full_snapshot_archives_dir)
            {
                match next_snapshot_type {
                    NextSnapshotType::FullSnapshot => {
                        if full_snapshot_archive_info.slot() >= last_slot {
                            break NextSnapshotResult::FullSnapshot(full_snapshot_archive_info);
                        }
                    }
                    NextSnapshotType::IncrementalAndFullSnapshot => {
                        if let Some(incremental_snapshot_archive_info) =
                            snapshot_utils::get_highest_incremental_snapshot_archive_info(
                                incremental_snapshot_archives_dir.as_ref().unwrap(),
                                full_snapshot_archive_info.slot(),
                            )
                        {
                            if incremental_snapshot_archive_info.slot() >= last_slot {
                                break NextSnapshotResult::IncrementalAndFullSnapshot(
                                    incremental_snapshot_archive_info,
                                    full_snapshot_archive_info,
                                );
                            }
                        }
                    }
                }
            }
            if let Some(max_wait_duration) = max_wait_duration {
                assert!(
                    timer.elapsed() < max_wait_duration,
                    "Waiting for next {next_snapshot_type:?} snapshot exceeded the {max_wait_duration:?} maximum wait duration!",
                );
            }
            sleep(Duration::from_secs(5));
        };
        trace!(
            "Waited {:?} for next snapshot archive: {:?}",
            timer.elapsed(),
            next_snapshot,
        );

        next_snapshot
    }
}

#[derive(Debug)]
pub enum NextSnapshotType {
    FullSnapshot,
    IncrementalAndFullSnapshot,
}

#[derive(Debug)]
pub enum NextSnapshotResult {
    FullSnapshot(FullSnapshotArchiveInfo),
    IncrementalAndFullSnapshot(IncrementalSnapshotArchiveInfo, FullSnapshotArchiveInfo),
}

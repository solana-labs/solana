use crate::snapshot_utils::ArchiveFormat;
use crate::snapshot_utils::SnapshotVersion;
use solana_sdk::clock::Slot;
use std::path::PathBuf;

/// Snapshot configuration and runtime information
#[derive(Clone, Debug)]
pub struct SnapshotConfig {
    /// Generate a new full snapshot archive every this many slots
    pub full_snapshot_archive_interval_slots: Slot,

    /// Generate a new incremental snapshot archive every this many slots
    pub incremental_snapshot_archive_interval_slots: Slot,

    /// Where to store the latest packaged snapshot archives
    pub snapshot_package_output_path: PathBuf,

    /// Where to place the bank snapshots for recent slots
    pub snapshot_path: PathBuf,

    /// The archive format to use for snapshots
    pub archive_format: ArchiveFormat,

    /// Snapshot version to generate
    pub snapshot_version: SnapshotVersion,

    /// Maximum number of full snapshot archives to retain
    pub maximum_snapshots_to_retain: usize,
}

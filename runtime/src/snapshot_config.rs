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

    /// Path to the directory where snapshot archives are stored
    pub snapshot_archives_dir: PathBuf,

    /// Path to the directory where bank snapshots are stored
    pub bank_snapshots_dir: PathBuf,

    /// The archive format to use for snapshots
    pub archive_format: ArchiveFormat,

    /// Snapshot version to generate
    pub snapshot_version: SnapshotVersion,

    /// Maximum number of full snapshot archives to retain
    pub maximum_full_snapshot_archives_to_retain: usize,

    /// Maximum number of incremental snapshot archives to retain
    /// NOTE: Incremental snapshots will only be kept for the latest full snapshot
    pub maximum_incremental_snapshot_archives_to_retain: usize,
}

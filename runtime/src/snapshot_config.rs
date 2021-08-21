use crate::snapshot_utils::ArchiveFormat;
use crate::snapshot_utils::SnapshotVersion;
use solana_sdk::clock::Slot;
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

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
    pub maximum_snapshots_to_retain: usize,

    /// Runtime information of the last full snapshot slot
    pub last_full_snapshot_slot: LastFullSnapshotSlot,
}

pub type LastFullSnapshotSlot = Arc<RwLock<Option<Slot>>>;

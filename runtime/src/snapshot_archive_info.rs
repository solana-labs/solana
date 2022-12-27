//! Information about snapshot archives

use {
    crate::{
        snapshot_hash::SnapshotHash,
        snapshot_utils::{self, ArchiveFormat, Result},
    },
    solana_sdk::clock::Slot,
    std::{cmp::Ordering, path::PathBuf},
};

/// Trait to query the snapshot archive information
pub trait SnapshotArchiveInfoGetter {
    fn snapshot_archive_info(&self) -> &SnapshotArchiveInfo;

    fn path(&self) -> &PathBuf {
        &self.snapshot_archive_info().path
    }

    fn slot(&self) -> Slot {
        self.snapshot_archive_info().slot
    }

    fn hash(&self) -> &SnapshotHash {
        &self.snapshot_archive_info().hash
    }

    fn archive_format(&self) -> ArchiveFormat {
        self.snapshot_archive_info().archive_format
    }

    fn is_remote(&self) -> bool {
        self.snapshot_archive_info()
            .path
            .parent()
            .map_or(false, |p| {
                p.ends_with(snapshot_utils::SNAPSHOT_ARCHIVE_DOWNLOAD_DIR)
            })
    }
}

/// Common information about a snapshot archive
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct SnapshotArchiveInfo {
    /// Path to the snapshot archive file
    pub path: PathBuf,

    /// Slot that the snapshot was made
    pub slot: Slot,

    /// Hash for the snapshot
    pub hash: SnapshotHash,

    /// Archive format for the snapshot file
    pub archive_format: ArchiveFormat,
}

/// Information about a full snapshot archive: its path, slot, hash, and archive format
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct FullSnapshotArchiveInfo(SnapshotArchiveInfo);

impl FullSnapshotArchiveInfo {
    /// Parse the path to a full snapshot archive and return a new `FullSnapshotArchiveInfo`
    pub fn new_from_path(path: PathBuf) -> Result<Self> {
        let filename = snapshot_utils::path_to_file_name_str(path.as_path())?;
        let (slot, hash, archive_format) =
            snapshot_utils::parse_full_snapshot_archive_filename(filename)?;

        Ok(Self::new(SnapshotArchiveInfo {
            path,
            slot,
            hash,
            archive_format,
        }))
    }

    pub(crate) fn new(snapshot_archive_info: SnapshotArchiveInfo) -> Self {
        Self(snapshot_archive_info)
    }
}

impl SnapshotArchiveInfoGetter for FullSnapshotArchiveInfo {
    fn snapshot_archive_info(&self) -> &SnapshotArchiveInfo {
        &self.0
    }
}

impl PartialOrd for FullSnapshotArchiveInfo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Order `FullSnapshotArchiveInfo` by slot (ascending), which practically is sorting chronologically
impl Ord for FullSnapshotArchiveInfo {
    fn cmp(&self, other: &Self) -> Ordering {
        self.slot().cmp(&other.slot())
    }
}

/// Information about an incremental snapshot archive: its path, slot, base slot, hash, and archive format
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct IncrementalSnapshotArchiveInfo {
    /// The slot that the incremental snapshot was based from.  This is the same as the full
    /// snapshot slot used when making the incremental snapshot.
    base_slot: Slot,

    /// Use the `SnapshotArchiveInfo` struct for the common fields: path, slot, hash, and
    /// archive_format, but as they pertain to the incremental snapshot.
    inner: SnapshotArchiveInfo,
}

impl IncrementalSnapshotArchiveInfo {
    /// Parse the path to an incremental snapshot archive and return a new `IncrementalSnapshotArchiveInfo`
    pub fn new_from_path(path: PathBuf) -> Result<Self> {
        let filename = snapshot_utils::path_to_file_name_str(path.as_path())?;
        let (base_slot, slot, hash, archive_format) =
            snapshot_utils::parse_incremental_snapshot_archive_filename(filename)?;

        Ok(Self::new(
            base_slot,
            SnapshotArchiveInfo {
                path,
                slot,
                hash,
                archive_format,
            },
        ))
    }

    pub(crate) fn new(base_slot: Slot, snapshot_archive_info: SnapshotArchiveInfo) -> Self {
        Self {
            base_slot,
            inner: snapshot_archive_info,
        }
    }

    pub fn base_slot(&self) -> Slot {
        self.base_slot
    }
}

impl SnapshotArchiveInfoGetter for IncrementalSnapshotArchiveInfo {
    fn snapshot_archive_info(&self) -> &SnapshotArchiveInfo {
        &self.inner
    }
}

impl PartialOrd for IncrementalSnapshotArchiveInfo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Order `IncrementalSnapshotArchiveInfo` by base slot (ascending), then slot (ascending), which
// practically is sorting chronologically
impl Ord for IncrementalSnapshotArchiveInfo {
    fn cmp(&self, other: &Self) -> Ordering {
        self.base_slot()
            .cmp(&other.base_slot())
            .then(self.slot().cmp(&other.slot()))
    }
}

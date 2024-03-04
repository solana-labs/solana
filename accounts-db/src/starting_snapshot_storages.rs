use {crate::accounts_db::AccountStorageEntry, std::sync::Arc};

/// Snapshot storages that the node loaded from
///
/// This is used to support fastboot.  Since fastboot reuses existing storages, we must carefully
/// handle the storages used to load at startup.  If we do not handle these storages properly,
/// restarting from the same local state (i.e. bank snapshot) may fail.
#[derive(Debug)]
pub enum StartingSnapshotStorages {
    /// Starting from genesis has no storages yet
    Genesis,
    /// Starting from a snapshot archive always extracts the storages from the archive, so no
    /// special handling is necessary to preserve them.
    Archive,
    /// Starting from local state must preserve the loaded storages.  These storages must *not* be
    /// recycled or removed prior to taking the next snapshot, otherwise restarting from the same
    /// bank snapshot may fail.
    Fastboot(Vec<Arc<AccountStorageEntry>>),
}

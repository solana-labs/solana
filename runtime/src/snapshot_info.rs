//! Shared runtime information about snapshots
//!
//! Snapshot information is needed at runtime in multiple threads.  Currently, this is mainly to
//! prevent cleaning of `AccountsDb` _past_ the last full snapshot slot.  However, snapshots are
//! taken in a separate thread, and `AccountsDb` didn't have a way get that information.
//!
//! This module was created to solve that problem.  Now, all threads that need runtime snapshot
//! information can have a `SyncSnapshotInfo` object to get/set the information they need.
//!
//! **NOTE** Client should use `SyncSnapshotInfo` and not `SnapshotInfo`, since the later does not
//! include synchronization (and the whole point of this module is to share snapshot information
//! between threads).
//!
//! **NOTE** There are helper functions to handle the common case of getting/setting snapshot
//! information when you have a `Option<SyncSnapshotInfo>`.

use solana_sdk::clock::Slot;
use std::sync::{Arc, RwLock};

/// Runtime information about snapshots
///
/// **NOTE** Clients should use `SyncSnapshotInfo` instead!
///
/// Information about the snapshots is needed in multiple threads at different times; for example,
/// `AccountsDb` (and `Bank` via `AccountsDb`) need to ensure cleaning does not happen past the
/// `last_full_snapshot_slot`.  Whereas `snapshot_utils`, `serde_snapshot`, `AccountsBackgroundService`, and
/// `SnapshotPackagerService` need to be able to set these values when snapshots are taken.
///
/// Share this object between all interested parties to ensure a consistent view of the snapshot
/// information.
#[derive(Debug)]
pub struct SnapshotInfo {
    /// The slot when the last full snapshot was taken.  Can be `None` if there are no full
    /// snapshots yet.
    last_full_snapshot_slot: Option<Slot>,

    /// The slot when the last incremental snapshot was taken.  Can be `None` if there are no
    /// incremental snapshots yet.  It is required that this incremental snapshot's base slot is
    /// the same as the `last_full_snapshot_slot`.
    last_incremental_snapshot_slot: Option<Slot>,
}

/// Snapshot runtime information, wrapped to make it multi-thread safe
pub type SyncSnapshotInfo = Arc<RwLock<SnapshotInfo>>;

impl Default for SnapshotInfo {
    fn default() -> Self {
        Self {
            last_full_snapshot_slot: None,
            last_incremental_snapshot_slot: None,
        }
    }
}

impl SnapshotInfo {
    /// Get the last full snapshot slot
    pub fn last_full_snapshot_slot(&self) -> Option<Slot> {
        self.last_full_snapshot_slot
    }

    /// Set the last full snapshot slot
    pub fn set_last_full_snapshot_slot(&mut self, slot: Slot) {
        self.last_full_snapshot_slot = Some(slot)
    }

    /// Get the last incremental snapshot slot
    pub fn last_incremental_snapshot_slot(&self) -> Option<Slot> {
        self.last_incremental_snapshot_slot
    }

    /// Set the last incremental snapshot slot
    pub fn set_last_incremental_snapshot_slot(&mut self, slot: Slot) {
        self.last_incremental_snapshot_slot = Some(slot)
    }
}

/// Helper function to get the last full snapshot slot from an option-and-sync-wrapped SnapshotInfo
pub fn get_last_full_snapshot_slot(snapshot_info: Option<&SyncSnapshotInfo>) -> Option<Slot> {
    snapshot_info
        .map(|sync_snapshot_info| sync_snapshot_info.read().unwrap().last_full_snapshot_slot())
        .flatten()
}

/// Helper function to set the last full snapshot slot from an option-and-sync-wrapped SnapshotInfo
pub fn set_last_full_snapshot_slot(snapshot_info: Option<&SyncSnapshotInfo>, slot: Slot) {
    if let Some(sync_snapshot_info) = snapshot_info {
        sync_snapshot_info
            .write()
            .unwrap()
            .set_last_full_snapshot_slot(slot)
    }
}

/// Helper function to get the last incremental snapshot slot from an option-and-sync-wrapped SnapshotInfo
pub fn get_last_incremental_snapshot_slot(
    snapshot_info: Option<&SyncSnapshotInfo>,
) -> Option<Slot> {
    snapshot_info
        .map(|sync_snapshot_info| {
            sync_snapshot_info
                .read()
                .unwrap()
                .last_incremental_snapshot_slot()
        })
        .flatten()
}

/// Helper function to set the last incremental snapshot slot from an option-and-sync-wrapped SnapshotInfo
pub fn set_last_incremental_snapshot_slot(snapshot_info: Option<&SyncSnapshotInfo>, slot: Slot) {
    if let Some(sync_snapshot_info) = snapshot_info {
        sync_snapshot_info
            .write()
            .unwrap()
            .set_last_incremental_snapshot_slot(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_synchronize_two_threads() {
        const STOP_COUNT: Slot = 10;
        let snapshot_info = SyncSnapshotInfo::default();

        // Loop on the last full snapshot slot, while updating the last incremental snapshot slot
        let snapshot_info1 = snapshot_info.clone();
        let handle1 = std::thread::spawn(move || {
            let mut count = 0;
            loop {
                if let Some(last_full_snapshot_slot) =
                    get_last_full_snapshot_slot(Some(&snapshot_info1))
                {
                    if last_full_snapshot_slot >= STOP_COUNT {
                        break;
                    }
                }

                set_last_incremental_snapshot_slot(Some(&snapshot_info1), count);
                count += 1;
            }
        });

        // Loop on the last incremental snapshot slot, while updating the last full snapshot slot
        let snapshot_info2 = snapshot_info.clone();
        let handle2 = std::thread::spawn(move || {
            let mut count = 0;
            loop {
                if let Some(last_incremental_snapshot_slot) =
                    get_last_incremental_snapshot_slot(Some(&snapshot_info2))
                {
                    if last_incremental_snapshot_slot >= STOP_COUNT {
                        break;
                    }
                }

                set_last_full_snapshot_slot(Some(&snapshot_info2), count);
                count += 1;
            }
        });

        handle1.join().unwrap();
        handle2.join().unwrap();
    }
}

//! Shared runtime information about snapshots
//!
//! Snapshot information is needed at runtime in multiple threads.  Currently, this is used to
//! prevent cleaning of zero-lamport accounts in `AccountsDb` _past_ the last full snapshot slot.
//! However, snapshots are taken in a separate thread, and previously `AccountsDb` didn't have a
//! way to get that information.
//!
//! This module was created to solve that problem.  Now, all threads that need runtime snapshot
//! information can have a `SyncSnapshotRuntimeInfo` object to get/set the information they need.
//!
//! **NOTE** Client should use `SyncSnapshotRuntimeInfo` and not `SnapshotRuntimeInfo`, since the later does not
//! include synchronization (and the whole point of this module is to share snapshot information
//! between threads).
//!
//! **NOTE** There are helper functions to handle the common case of getting/setting snapshot
//! information when you have a `Option<SyncSnapshotRuntimeInfo>`.

use solana_sdk::clock::Slot;
use std::sync::{Arc, RwLock};

/// Runtime information about snapshots
///
/// **NOTE** Clients should use `SyncSnapshotRuntimeInfo` instead!
///
/// Information about the snapshots is needed in multiple threads at different times; for example,
/// `AccountsDb` (and `Bank` via `AccountsDb`) need to ensure cleaning does not happen past the
/// `last_full_snapshot_slot`.  Whereas `snapshot_utils`, `serde_snapshot`, `AccountsBackgroundService`, and
/// `SnapshotPackagerService` need to be able to set these values when snapshots are taken.
///
/// Share this object between all interested parties to ensure a consistent view of the snapshot
/// information.
#[derive(Debug)]
pub struct SnapshotRuntimeInfo {
    /// The slot when the last full snapshot was taken.  Can be `None` if there are no full
    /// snapshots yet.
    last_full_snapshot_slot: Option<Slot>,

    /// The slot when the last incremental snapshot was taken.  Can be `None` if there are no
    /// incremental snapshots yet.  It is required that this incremental snapshot's base slot is
    /// the same as the `last_full_snapshot_slot`.
    last_incremental_snapshot_slot: Option<Slot>,
}

/// Snapshot runtime information, wrapped to make it multi-thread safe
pub type SyncSnapshotRuntimeInfo = Arc<RwLock<SnapshotRuntimeInfo>>;

impl Default for SnapshotRuntimeInfo {
    fn default() -> Self {
        Self {
            last_full_snapshot_slot: None,
            last_incremental_snapshot_slot: None,
        }
    }
}

impl SnapshotRuntimeInfo {
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

/// Helper function to get the last full snapshot slot from an option-and-sync-wrapped SnapshotRuntimeInfo
pub fn get_last_full_snapshot_slot(
    snapshot_runtime_info: Option<&SyncSnapshotRuntimeInfo>,
) -> Option<Slot> {
    snapshot_runtime_info
        .map(|sync_snapshot_runtime_info| {
            sync_snapshot_runtime_info
                .read()
                .unwrap()
                .last_full_snapshot_slot()
        })
        .flatten()
}

/// Helper function to set the last full snapshot slot from an option-and-sync-wrapped SnapshotRuntimeInfo
pub fn set_last_full_snapshot_slot(
    snapshot_runtime_info: Option<&SyncSnapshotRuntimeInfo>,
    slot: Slot,
) {
    if let Some(sync_snapshot_runtime_info) = snapshot_runtime_info {
        sync_snapshot_runtime_info
            .write()
            .unwrap()
            .set_last_full_snapshot_slot(slot)
    }
}

/// Helper function to get the last incremental snapshot slot from an option-and-sync-wrapped SnapshotRuntimeInfo
pub fn get_last_incremental_snapshot_slot(
    snapshot_runtime_info: Option<&SyncSnapshotRuntimeInfo>,
) -> Option<Slot> {
    snapshot_runtime_info
        .map(|sync_snapshot_runtime_info| {
            sync_snapshot_runtime_info
                .read()
                .unwrap()
                .last_incremental_snapshot_slot()
        })
        .flatten()
}

/// Helper function to set the last incremental snapshot slot from an option-and-sync-wrapped SnapshotRuntimeInfo
pub fn set_last_incremental_snapshot_slot(
    snapshot_runtime_info: Option<&SyncSnapshotRuntimeInfo>,
    slot: Slot,
) {
    if let Some(sync_snapshot_runtime_info) = snapshot_runtime_info {
        sync_snapshot_runtime_info
            .write()
            .unwrap()
            .set_last_incremental_snapshot_slot(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Test to ensure that SyncSnapshotRuntimeInfo can be used with the helper functions in two threads.
    fn test_snapshot_runtime_info_can_synchronize_two_threads() {
        const STOP_COUNT: Slot = 10;
        let snapshot_runtime_info = SyncSnapshotRuntimeInfo::default();

        // Loop on the last full snapshot slot, while updating the last incremental snapshot slot
        let snapshot_runtime_info1 = snapshot_runtime_info.clone();
        let handle1 = std::thread::spawn(move || {
            let mut count = 0;
            loop {
                set_last_incremental_snapshot_slot(Some(&snapshot_runtime_info1), count);

                if let Some(last_full_snapshot_slot) =
                    get_last_full_snapshot_slot(Some(&snapshot_runtime_info1))
                {
                    if last_full_snapshot_slot >= STOP_COUNT && count >= STOP_COUNT {
                        break;
                    }
                }

                count += 1;
            }
        });

        // Loop on the last incremental snapshot slot, while updating the last full snapshot slot
        let snapshot_runtime_info2 = snapshot_runtime_info.clone();
        let handle2 = std::thread::spawn(move || {
            let mut count = 0;
            loop {
                set_last_full_snapshot_slot(Some(&snapshot_runtime_info2), count);

                if let Some(last_incremental_snapshot_slot) =
                    get_last_incremental_snapshot_slot(Some(&snapshot_runtime_info2))
                {
                    if last_incremental_snapshot_slot >= STOP_COUNT && count >= STOP_COUNT {
                        break;
                    }
                }

                count += 1;
            }
        });

        handle1.join().unwrap();
        handle2.join().unwrap();

        assert!(get_last_full_snapshot_slot(Some(&snapshot_runtime_info)).unwrap() >= STOP_COUNT);
        assert!(
            get_last_incremental_snapshot_slot(Some(&snapshot_runtime_info)).unwrap() >= STOP_COUNT
        );
    }
}

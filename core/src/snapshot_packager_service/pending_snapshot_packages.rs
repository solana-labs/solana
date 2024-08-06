use {
    solana_runtime::snapshot_package::{
        cmp_snapshot_packages_by_priority, SnapshotKind, SnapshotPackage,
    },
    std::cmp::Ordering::Greater,
};

/// Snapshot packages that are pending for archival
#[derive(Debug, Default)]
pub struct PendingSnapshotPackages {
    full: Option<SnapshotPackage>,
    incremental: Option<SnapshotPackage>,
}

impl PendingSnapshotPackages {
    /// Adds `snapshot_package` as a pending snapshot package
    ///
    /// This will overwrite currently-pending in-kind packages.
    ///
    /// Note: This function will panic if `snapshot_package` is *older*
    /// than any currently-pending in-kind packages.
    pub fn push(&mut self, snapshot_package: SnapshotPackage) {
        match snapshot_package.snapshot_kind {
            SnapshotKind::FullSnapshot => {
                if let Some(pending_full_snapshot_package) = self.full.as_ref() {
                    // snapshots are monotonically increasing; only overwrite *old* packages
                    assert!(pending_full_snapshot_package
                        .snapshot_kind
                        .is_full_snapshot());
                    assert_eq!(
                        cmp_snapshot_packages_by_priority(
                            &snapshot_package,
                            pending_full_snapshot_package,
                        ),
                        Greater,
                        "full snapshot package must be newer than pending package, \
                         old: {pending_full_snapshot_package:?}, new: {snapshot_package:?}",
                    );
                    info!(
                        "overwrote pending full snapshot package, old slot: {}, new slot: {}",
                        pending_full_snapshot_package.slot, snapshot_package.slot,
                    );
                }
                self.full = Some(snapshot_package)
            }
            SnapshotKind::IncrementalSnapshot(_) => {
                if let Some(pending_incremental_snapshot_package) = self.incremental.as_ref() {
                    // snapshots are monotonically increasing; only overwrite *old* packages
                    assert!(pending_incremental_snapshot_package
                        .snapshot_kind
                        .is_incremental_snapshot());
                    assert_eq!(
                        cmp_snapshot_packages_by_priority(
                            &snapshot_package,
                            pending_incremental_snapshot_package,
                        ),
                        Greater,
                        "incremental snapshot package must be newer than pending package, \
                         old: {pending_incremental_snapshot_package:?}, new: {snapshot_package:?}",
                    );
                    info!(
                        "overwrote pending incremental snapshot package, old slot: {}, new slot: {}",
                        pending_incremental_snapshot_package.slot, snapshot_package.slot,
                    );
                }
                self.incremental = Some(snapshot_package)
            }
        }
    }

    /// Returns the next pending snapshot package to handle
    pub fn pop(&mut self) -> Option<SnapshotPackage> {
        let pending_full = self.full.take();
        let pending_incremental = self.incremental.take();
        match (pending_full, pending_incremental) {
            (Some(pending_full), pending_incremental) => {
                // If there is a pending incremental snapshot package, check its slot.
                // If its slot is greater than the full snapshot package's,
                // re-enqueue it, otherwise drop it.
                // Note that it is *not supported* to handle incremental snapshots with
                // slots *older* than the latest full snapshot.  This is why we do not
                // re-enqueue every incremental snapshot.
                if let Some(pending_incremental) = pending_incremental {
                    let SnapshotKind::IncrementalSnapshot(base_slot) =
                        &pending_incremental.snapshot_kind
                    else {
                        panic!(
                            "the pending incremental snapshot package must be of kind \
                             IncrementalSnapshot, but instead was {pending_incremental:?}",
                        );
                    };
                    if pending_incremental.slot > pending_full.slot
                        && *base_slot >= pending_full.slot
                    {
                        self.incremental = Some(pending_incremental);
                    }
                }

                assert!(pending_full.snapshot_kind.is_full_snapshot());
                Some(pending_full)
            }
            (None, Some(pending_incremental)) => {
                assert!(pending_incremental.snapshot_kind.is_incremental_snapshot());
                Some(pending_incremental)
            }
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_runtime::snapshot_package::{SnapshotKind, SnapshotPackage},
        solana_sdk::clock::Slot,
    };

    fn new(snapshot_kind: SnapshotKind, slot: Slot) -> SnapshotPackage {
        SnapshotPackage {
            snapshot_kind,
            slot,
            block_height: slot,
            ..SnapshotPackage::default_for_tests()
        }
    }
    fn new_full(slot: Slot) -> SnapshotPackage {
        new(SnapshotKind::FullSnapshot, slot)
    }
    fn new_incr(slot: Slot, base: Slot) -> SnapshotPackage {
        new(SnapshotKind::IncrementalSnapshot(base), slot)
    }

    #[test]
    fn test_default() {
        let pending_snapshot_packages = PendingSnapshotPackages::default();
        assert!(pending_snapshot_packages.full.is_none());
        assert!(pending_snapshot_packages.incremental.is_none());
    }

    #[test]
    fn test_push() {
        let mut pending_snapshot_packages = PendingSnapshotPackages::default();

        // ensure we can push full snapshot packages
        let slot = 100;
        pending_snapshot_packages.push(new_full(slot));
        assert_eq!(pending_snapshot_packages.full.as_ref().unwrap().slot, slot);
        assert!(pending_snapshot_packages.incremental.is_none());

        // ensure we can overwrite full snapshot packages
        let slot = slot + 100;
        pending_snapshot_packages.push(new_full(slot));
        assert_eq!(pending_snapshot_packages.full.as_ref().unwrap().slot, slot);
        assert!(pending_snapshot_packages.incremental.is_none());

        // ensure we can push incremental packages
        let full_slot = slot;
        let slot = full_slot + 10;
        pending_snapshot_packages.push(new_incr(slot, full_slot));
        assert_eq!(
            pending_snapshot_packages.full.as_ref().unwrap().slot,
            full_slot,
        );
        assert_eq!(
            pending_snapshot_packages.incremental.as_ref().unwrap().slot,
            slot,
        );

        // ensure we can overwrite incremental packages
        let slot = slot + 10;
        pending_snapshot_packages.push(new_incr(slot, full_slot));
        assert_eq!(
            pending_snapshot_packages.full.as_ref().unwrap().slot,
            full_slot,
        );
        assert_eq!(
            pending_snapshot_packages.incremental.as_ref().unwrap().slot,
            slot,
        );

        // ensure pushing a full package doesn't affect the incremental package
        // (we already tested above that pushing an incremental doesn't affect the full)
        let incremental_slot = slot;
        let slot = full_slot + 100;
        pending_snapshot_packages.push(new_full(slot));
        assert_eq!(pending_snapshot_packages.full.as_ref().unwrap().slot, slot);
        assert_eq!(
            pending_snapshot_packages.incremental.as_ref().unwrap().slot,
            incremental_slot,
        );
    }

    #[test]
    #[should_panic(expected = "full snapshot package must be newer than pending package")]
    fn test_push_older_full() {
        let slot = 100;
        let mut pending_snapshot_packages = PendingSnapshotPackages {
            full: Some(new_full(slot)),
            incremental: None,
        };

        // pushing an older full should panic
        pending_snapshot_packages.push(new_full(slot - 1));
    }

    #[test]
    #[should_panic(expected = "incremental snapshot package must be newer than pending package")]
    fn test_push_older_incremental() {
        let base = 100;
        let slot = base + 20;
        let mut pending_snapshot_packages = PendingSnapshotPackages {
            full: None,
            incremental: Some(new_incr(slot, base)),
        };

        // pushing an older incremental should panic
        pending_snapshot_packages.push(new_incr(slot - 1, base));
    }

    #[test]
    fn test_pop() {
        let mut pending_snapshot_packages = PendingSnapshotPackages::default();

        // ensure we can call pop when there are no pending packages
        assert!(pending_snapshot_packages.pop().is_none());

        // ensure pop returns full when there's only a full
        let slot = 100;
        pending_snapshot_packages.full = Some(new_full(slot));
        pending_snapshot_packages.incremental = None;
        let snapshot_package = pending_snapshot_packages.pop().unwrap();
        assert!(snapshot_package.snapshot_kind.is_full_snapshot());
        assert_eq!(snapshot_package.slot, slot);

        // ensure pop returns incremental when there's only an incremental
        let base = 100;
        let slot = base + 10;
        pending_snapshot_packages.full = None;
        pending_snapshot_packages.incremental = Some(new_incr(slot, base));
        let snapshot_package = pending_snapshot_packages.pop().unwrap();
        assert!(snapshot_package.snapshot_kind.is_incremental_snapshot());
        assert_eq!(snapshot_package.slot, slot);

        // ensure pop returns full when there's both a full and newer incremental
        let full_slot = 100;
        let incr_slot = full_slot + 10;
        pending_snapshot_packages.full = Some(new_full(full_slot));
        pending_snapshot_packages.incremental = Some(new_incr(incr_slot, full_slot));
        let snapshot_package = pending_snapshot_packages.pop().unwrap();
        assert!(snapshot_package.snapshot_kind.is_full_snapshot());
        assert_eq!(snapshot_package.slot, full_slot);

        // ..and then the second pop returns the incremental
        let snapshot_package = pending_snapshot_packages.pop().unwrap();
        assert!(snapshot_package.snapshot_kind.is_incremental_snapshot());
        assert_eq!(snapshot_package.slot, incr_slot);

        // but, if there's a full and *older* incremental, pop should return
        // the full and *not* re-enqueue the incremental
        let full_slot = 200;
        let incr_slot = full_slot - 10;
        pending_snapshot_packages.full = Some(new_full(full_slot));
        pending_snapshot_packages.incremental = Some(new_incr(incr_slot, full_slot));
        let snapshot_package = pending_snapshot_packages.pop().unwrap();
        assert!(snapshot_package.snapshot_kind.is_full_snapshot());
        assert_eq!(snapshot_package.slot, full_slot);
        assert!(pending_snapshot_packages.incremental.is_none());
    }

    #[test]
    #[should_panic]
    fn test_pop_invalid_pending_full() {
        let mut pending_snapshot_packages = PendingSnapshotPackages {
            full: Some(new_incr(110, 100)), // <-- invalid! `full` is IncrementalSnapshot
            incremental: None,
        };
        pending_snapshot_packages.pop();
    }

    #[test]
    #[should_panic]
    fn test_pop_invalid_pending_incremental() {
        let mut pending_snapshot_packages = PendingSnapshotPackages {
            full: None,
            incremental: Some(new_full(100)), // <-- invalid! `incremental` is FullSnapshot
        };
        pending_snapshot_packages.pop();
    }
}

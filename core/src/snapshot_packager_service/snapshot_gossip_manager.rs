use {
    solana_gossip::cluster_info::ClusterInfo,
    solana_runtime::{
        snapshot_hash::{
            FullSnapshotHash, IncrementalSnapshotHash, SnapshotHash, StartingSnapshotHashes,
        },
        snapshot_package::SnapshotKind,
    },
    solana_sdk::{clock::Slot, hash::Hash},
    std::sync::Arc,
};

/// Manage pushing snapshot hash information to gossip
pub struct SnapshotGossipManager {
    cluster_info: Arc<ClusterInfo>,
    latest_snapshot_hashes: Option<LatestSnapshotHashes>,
}

impl SnapshotGossipManager {
    /// Construct a new SnapshotGossipManager and push any
    /// starting snapshot hashes to the cluster via CRDS
    #[must_use]
    pub fn new(
        cluster_info: Arc<ClusterInfo>,
        starting_snapshot_hashes: Option<StartingSnapshotHashes>,
    ) -> Self {
        let mut this = SnapshotGossipManager {
            cluster_info,
            latest_snapshot_hashes: None,
        };
        if let Some(starting_snapshot_hashes) = starting_snapshot_hashes {
            this.push_starting_snapshot_hashes(starting_snapshot_hashes);
        }
        this
    }

    /// Push starting snapshot hashes to the cluster via CRDS
    fn push_starting_snapshot_hashes(&mut self, starting_snapshot_hashes: StartingSnapshotHashes) {
        self.update_latest_full_snapshot_hash(starting_snapshot_hashes.full);
        if let Some(starting_incremental_snapshot_hash) = starting_snapshot_hashes.incremental {
            self.update_latest_incremental_snapshot_hash(
                starting_incremental_snapshot_hash,
                starting_snapshot_hashes.full.0 .0,
            );
        }
        self.push_latest_snapshot_hashes_to_cluster();
    }

    /// Push new snapshot hash to the cluster via CRDS
    pub fn push_snapshot_hash(
        &mut self,
        snapshot_kind: SnapshotKind,
        snapshot_hash: (Slot, SnapshotHash),
    ) {
        match snapshot_kind {
            SnapshotKind::FullSnapshot => {
                self.push_full_snapshot_hash(FullSnapshotHash(snapshot_hash));
            }
            SnapshotKind::IncrementalSnapshot(base_slot) => {
                self.push_incremental_snapshot_hash(
                    IncrementalSnapshotHash(snapshot_hash),
                    base_slot,
                );
            }
        }
    }

    /// Push new full snapshot hash to the cluster via CRDS
    fn push_full_snapshot_hash(&mut self, full_snapshot_hash: FullSnapshotHash) {
        self.update_latest_full_snapshot_hash(full_snapshot_hash);
        self.push_latest_snapshot_hashes_to_cluster();
    }

    /// Push new incremental snapshot hash to the cluster via CRDS
    fn push_incremental_snapshot_hash(
        &mut self,
        incremental_snapshot_hash: IncrementalSnapshotHash,
        base_slot: Slot,
    ) {
        self.update_latest_incremental_snapshot_hash(incremental_snapshot_hash, base_slot);
        self.push_latest_snapshot_hashes_to_cluster();
    }

    /// Update the latest snapshot hashes with a new full snapshot
    fn update_latest_full_snapshot_hash(&mut self, full_snapshot_hash: FullSnapshotHash) {
        self.latest_snapshot_hashes = Some(LatestSnapshotHashes {
            full: full_snapshot_hash,
            // If we've gotten a new full snapshot, we know there cannot be any
            // incremental snapshots yet (based on this full snapshot).
            incremental: None,
        });
    }

    /// Update the latest snapshot hashes with a new incremental snapshot
    fn update_latest_incremental_snapshot_hash(
        &mut self,
        incremental_snapshot_hash: IncrementalSnapshotHash,
        base_slot: Slot,
    ) {
        let latest_snapshot_hashes = self
            .latest_snapshot_hashes
            .as_mut()
            .expect("there must already be a full snapshot hash");
        assert_eq!(
            base_slot, latest_snapshot_hashes.full.0.0,
            "the incremental snapshot's base slot ({}) must match the latest full snapshot's slot ({})",
            base_slot, latest_snapshot_hashes.full.0.0,
        );
        latest_snapshot_hashes.incremental = Some(incremental_snapshot_hash);
    }

    /// Push the latest snapshot hashes to the cluster via CRDS
    fn push_latest_snapshot_hashes_to_cluster(&self) {
        let Some(latest_snapshot_hashes) = self.latest_snapshot_hashes.as_ref() else {
            return;
        };

        // Pushing snapshot hashes to the cluster should never fail.  The only error case is when
        // the length of the incremental hashes is too big, (and we send a maximum of one here).
        // If this call ever does error, it's a programmer bug!  Check to see what changed in
        // `push_snapshot_hashes()` and handle the new error condition here.
        self.cluster_info
            .push_snapshot_hashes(
                latest_snapshot_hashes.full.clone_for_crds(),
                latest_snapshot_hashes
                    .incremental
                    .iter()
                    .map(AsSnapshotHash::clone_for_crds)
                    .collect(),
            )
            .expect(
                "Bug! The programmer contract has changed for push_snapshot_hashes() \
                 and a new error case has been added that has not been handled here.",
            );
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct LatestSnapshotHashes {
    full: FullSnapshotHash,
    incremental: Option<IncrementalSnapshotHash>,
}

trait AsSnapshotHash {
    fn as_snapshot_hash(&self) -> &(Slot, SnapshotHash);

    /// Clones and maps a into what CRDS expects
    fn clone_for_crds(&self) -> (Slot, Hash) {
        let (slot, snapshot_hash) = self.as_snapshot_hash();
        (*slot, snapshot_hash.0)
    }
}

impl AsSnapshotHash for FullSnapshotHash {
    fn as_snapshot_hash(&self) -> &(Slot, SnapshotHash) {
        &self.0
    }
}

impl AsSnapshotHash for IncrementalSnapshotHash {
    fn as_snapshot_hash(&self) -> &(Slot, SnapshotHash) {
        &self.0
    }
}

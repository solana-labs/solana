use {
    solana_gossip::cluster_info::ClusterInfo,
    solana_runtime::{
        snapshot_hash::{
            FullSnapshotHash, FullSnapshotHashes, SnapshotHash, StartingSnapshotHashes,
        },
        snapshot_package::{retain_max_n_elements, SnapshotType},
    },
    solana_sdk::{clock::Slot, hash::Hash},
    std::sync::Arc,
};

/// Manage pushing snapshot hash information to gossip
pub struct SnapshotGossipManager {
    cluster_info: Arc<ClusterInfo>,
    latest_snapshot_hashes: Option<LatestSnapshotHashes>,
    max_full_snapshot_hashes: usize,
    legacy_full_snapshot_hashes: FullSnapshotHashes,
}

impl SnapshotGossipManager {
    /// Construct a new SnapshotGossipManager with empty snapshot hashes
    #[must_use]
    pub fn new(
        cluster_info: Arc<ClusterInfo>,
        max_full_snapshot_hashes: usize,
        _max_incremental_snapshot_hashes: usize,
    ) -> Self {
        SnapshotGossipManager {
            cluster_info,
            latest_snapshot_hashes: None,
            max_full_snapshot_hashes,
            legacy_full_snapshot_hashes: FullSnapshotHashes {
                hashes: Vec::default(),
            },
        }
    }

    /// Push any starting snapshot hashes to the cluster via CRDS
    pub fn push_starting_snapshot_hashes(
        &mut self,
        starting_snapshot_hashes: Option<StartingSnapshotHashes>,
    ) {
        let Some(starting_snapshot_hashes) = starting_snapshot_hashes else {
            return;
        };

        self.update_latest_full_snapshot_hash(starting_snapshot_hashes.full.hash);
        if let Some(starting_incremental_snapshot_hash) = starting_snapshot_hashes.incremental {
            self.update_latest_incremental_snapshot_hash(
                starting_incremental_snapshot_hash.hash,
                starting_incremental_snapshot_hash.base.0,
            );
        }
        self.push_latest_snapshot_hashes_to_cluster();

        // Handle legacy snapshot hashes here too
        // Once LegacySnapshotHashes are removed from CRDS, also remove them here
        self.push_legacy_full_snapshot_hash(starting_snapshot_hashes.full);
    }

    /// Push new snapshot hash to the cluster via CRDS
    pub fn push_snapshot_hash(
        &mut self,
        snapshot_type: SnapshotType,
        snapshot_hash: (Slot, SnapshotHash),
    ) {
        match snapshot_type {
            SnapshotType::FullSnapshot => {
                self.push_full_snapshot_hash(snapshot_hash);
            }
            SnapshotType::IncrementalSnapshot(base_slot) => {
                self.push_incremental_snapshot_hash(snapshot_hash, base_slot);
            }
        }
    }

    /// Push new full snapshot hash to the cluster via CRDS
    fn push_full_snapshot_hash(&mut self, full_snapshot_hash: (Slot, SnapshotHash)) {
        self.update_latest_full_snapshot_hash(full_snapshot_hash);
        self.push_latest_snapshot_hashes_to_cluster();

        // Handle legacy snapshot hashes here too
        // Once LegacySnapshotHashes are removed from CRDS, also remove them here
        self.push_legacy_full_snapshot_hash(FullSnapshotHash {
            hash: full_snapshot_hash,
        });
    }

    /// Push new incremental snapshot hash to the cluster via CRDS
    fn push_incremental_snapshot_hash(
        &mut self,
        incremental_snapshot_hash: (Slot, SnapshotHash),
        base_slot: Slot,
    ) {
        self.update_latest_incremental_snapshot_hash(incremental_snapshot_hash, base_slot);
        self.push_latest_snapshot_hashes_to_cluster();
    }

    /// Update the latest snapshot hashes with a new full snapshot
    fn update_latest_full_snapshot_hash(&mut self, full_snapshot_hash: (Slot, SnapshotHash)) {
        self.latest_snapshot_hashes = Some(LatestSnapshotHashes {
            full: full_snapshot_hash,
            incremental: None,
        });
    }

    /// Update the latest snapshot hashes with a new incremental snapshot
    fn update_latest_incremental_snapshot_hash(
        &mut self,
        incremental_snapshot_hash: (Slot, SnapshotHash),
        base_slot: Slot,
    ) {
        let latest_snapshot_hashes = self
            .latest_snapshot_hashes
            .as_mut()
            .expect("there must already be a full snapshot hash");
        assert_eq!(
            base_slot, latest_snapshot_hashes.full.0,
            "the incremental snapshot's base slot ({}) must match the latest full snapshot's slot ({})",
            base_slot, latest_snapshot_hashes.full.0,
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
                clone_hash_for_crds(&latest_snapshot_hashes.full),
                latest_snapshot_hashes
                    .incremental
                    .iter()
                    .map(clone_hash_for_crds)
                    .collect(),
            )
            .expect(
                "Bug! The programmer contract has changed for push_snapshot_hashes() \
                 and a new error case has been added that has not been handled here.",
            );
    }

    /// Add `full_snapshot_hash` to the vector of full snapshot hashes, then push that vector to
    /// the cluster via CRDS.
    fn push_legacy_full_snapshot_hash(&mut self, full_snapshot_hash: FullSnapshotHash) {
        self.legacy_full_snapshot_hashes
            .hashes
            .push(full_snapshot_hash.hash);

        retain_max_n_elements(
            &mut self.legacy_full_snapshot_hashes.hashes,
            self.max_full_snapshot_hashes,
        );

        self.cluster_info
            .push_legacy_snapshot_hashes(clone_hashes_for_crds(
                &self.legacy_full_snapshot_hashes.hashes,
            ));
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct LatestSnapshotHashes {
    full: (Slot, SnapshotHash),
    incremental: Option<(Slot, SnapshotHash)>,
}

/// Clones and maps snapshot hashes into what CRDS expects
fn clone_hashes_for_crds(hashes: &[(Slot, SnapshotHash)]) -> Vec<(Slot, Hash)> {
    hashes.iter().map(clone_hash_for_crds).collect()
}

/// Clones and maps a snapshot hash into what CRDS expects
fn clone_hash_for_crds(hash: &(Slot, SnapshotHash)) -> (Slot, Hash) {
    (hash.0, hash.1 .0)
}

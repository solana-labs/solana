use {
    solana_gossip::cluster_info::ClusterInfo,
    solana_runtime::{
        snapshot_hash::{
            FullSnapshotHash, FullSnapshotHashes, IncrementalSnapshotHash,
            IncrementalSnapshotHashes, SnapshotHash, StartingSnapshotHashes,
        },
        snapshot_package::{retain_max_n_elements, SnapshotType},
    },
    solana_sdk::{clock::Slot, hash::Hash},
    std::sync::Arc,
};

/// Manage pushing snapshot hash information to gossip
pub struct SnapshotGossipManager {
    cluster_info: Arc<ClusterInfo>,
    max_full_snapshot_hashes: usize,
    max_incremental_snapshot_hashes: usize,
    full_snapshot_hashes: FullSnapshotHashes,
    incremental_snapshot_hashes: IncrementalSnapshotHashes,
}

impl SnapshotGossipManager {
    /// Construct a new SnapshotGossipManager with empty snapshot hashes
    #[must_use]
    pub fn new(
        cluster_info: Arc<ClusterInfo>,
        max_full_snapshot_hashes: usize,
        max_incremental_snapshot_hashes: usize,
    ) -> Self {
        SnapshotGossipManager {
            cluster_info,
            max_full_snapshot_hashes,
            max_incremental_snapshot_hashes,
            full_snapshot_hashes: FullSnapshotHashes {
                hashes: Vec::default(),
            },
            incremental_snapshot_hashes: IncrementalSnapshotHashes {
                base: (Slot::default(), SnapshotHash(Hash::default())),
                hashes: Vec::default(),
            },
        }
    }

    /// If there were starting snapshot hashes, add those to their respective vectors, then push
    /// those vectors to the cluster via CRDS.
    pub fn push_starting_snapshot_hashes(
        &mut self,
        starting_snapshot_hashes: Option<StartingSnapshotHashes>,
    ) {
        if let Some(starting_snapshot_hashes) = starting_snapshot_hashes {
            let starting_full_snapshot_hash = starting_snapshot_hashes.full;
            self.push_full_snapshot_hash(starting_full_snapshot_hash);

            if let Some(starting_incremental_snapshot_hash) = starting_snapshot_hashes.incremental {
                self.push_incremental_snapshot_hash(starting_incremental_snapshot_hash);
            };
        }
    }

    /// Add `snapshot_hash` to its respective vector of hashes, then push that vector to the
    /// cluster via CRDS.
    pub fn push_snapshot_hash(
        &mut self,
        snapshot_type: SnapshotType,
        snapshot_hash: (Slot, SnapshotHash),
    ) {
        match snapshot_type {
            SnapshotType::FullSnapshot => {
                self.push_full_snapshot_hash(FullSnapshotHash {
                    hash: snapshot_hash,
                });
            }
            SnapshotType::IncrementalSnapshot(base_slot) => {
                let latest_full_snapshot_hash = *self.full_snapshot_hashes.hashes.last().unwrap();
                assert_eq!(
                    base_slot, latest_full_snapshot_hash.0,
                    "the incremental snapshot's base slot ({}) must match the latest full snapshot hash's slot ({})",
                    base_slot, latest_full_snapshot_hash.0,
                );
                self.push_incremental_snapshot_hash(IncrementalSnapshotHash {
                    base: latest_full_snapshot_hash,
                    hash: snapshot_hash,
                });
            }
        }
    }

    /// Add `full_snapshot_hash` to the vector of full snapshot hashes, then push that vector to
    /// the cluster via CRDS.
    fn push_full_snapshot_hash(&mut self, full_snapshot_hash: FullSnapshotHash) {
        self.full_snapshot_hashes
            .hashes
            .push(full_snapshot_hash.hash);

        retain_max_n_elements(
            &mut self.full_snapshot_hashes.hashes,
            self.max_full_snapshot_hashes,
        );

        self.cluster_info
            .push_legacy_snapshot_hashes(clone_hashes_for_crds(&self.full_snapshot_hashes.hashes));
    }

    /// Add `incremental_snapshot_hash` to the vector of incremental snapshot hashes, then push
    /// that vector to the cluster via CRDS.
    fn push_incremental_snapshot_hash(
        &mut self,
        incremental_snapshot_hash: IncrementalSnapshotHash,
    ) {
        // If the base snapshot hash is different from the one in IncrementalSnapshotHashes, then
        // that means the old incremental snapshot hashes are no longer valid, so clear them all
        // out.
        if incremental_snapshot_hash.base != self.incremental_snapshot_hashes.base {
            self.incremental_snapshot_hashes.hashes.clear();
            self.incremental_snapshot_hashes.base = incremental_snapshot_hash.base;
        }

        self.incremental_snapshot_hashes
            .hashes
            .push(incremental_snapshot_hash.hash);

        retain_max_n_elements(
            &mut self.incremental_snapshot_hashes.hashes,
            self.max_incremental_snapshot_hashes,
        );

        // Pushing incremental snapshot hashes to the cluster should never fail.  The only error
        // case is when the length of the hashes is too big, but we account for that with
        // `max_incremental_snapshot_hashes`.  If this call ever does error, it's a programmer bug!
        // Check to see what changed in `push_snapshot_hashes()` and handle the new
        // error condition here.
        self.cluster_info
            .push_snapshot_hashes(
                clone_hash_for_crds(&self.incremental_snapshot_hashes.base),
                clone_hashes_for_crds(&self.incremental_snapshot_hashes.hashes),
            )
            .expect(
                "Bug! The programmer contract has changed for push_snapshot_hashes() \
                 and a new error case has been added, which has not been handled here.",
            );
    }
}

/// Clones and maps snapshot hashes into what CRDS expects
fn clone_hashes_for_crds(hashes: &[(Slot, SnapshotHash)]) -> Vec<(Slot, Hash)> {
    hashes.iter().map(clone_hash_for_crds).collect()
}

/// Clones and maps a snapshot hash into what CRDS expects
fn clone_hash_for_crds(hash: &(Slot, SnapshotHash)) -> (Slot, Hash) {
    (hash.0, hash.1 .0)
}

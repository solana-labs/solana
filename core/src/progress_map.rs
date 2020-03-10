use crate::{cluster_info_vote_listener::SlotVoteTracker, consensus::StakeLockout};
use solana_ledger::blockstore_processor::{ConfirmationProgress, ConfirmationTiming};
use solana_runtime::bank::Bank;
use solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::{Arc, RwLock},
};

#[derive(Default)]
pub(crate) struct ReplaySlotStats(ConfirmationTiming);
impl std::ops::Deref for ReplaySlotStats {
    type Target = ConfirmationTiming;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ReplaySlotStats {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ReplaySlotStats {
    pub fn report_stats(&self, slot: Slot, num_entries: usize, num_shreds: u64) {
        datapoint_info!(
            "replay-slot-stats",
            ("slot", slot as i64, i64),
            ("fetch_entries_time", self.fetch_elapsed as i64, i64),
            (
                "fetch_entries_fail_time",
                self.fetch_fail_elapsed as i64,
                i64
            ),
            ("entry_verification_time", self.verify_elapsed as i64, i64),
            ("replay_time", self.replay_elapsed as i64, i64),
            (
                "replay_total_elapsed",
                self.started.elapsed().as_micros() as i64,
                i64
            ),
            ("total_entries", num_entries as i64, i64),
            ("total_shreds", num_shreds as i64, i64),
        );
    }
}

pub(crate) struct ForkProgress {
    pub(crate) is_dead: bool,
    pub(crate) fork_stats: ForkStats,
    pub(crate) propagated_stats: PropagatedStats,
    pub(crate) replay_stats: ReplaySlotStats,
    pub(crate) replay_progress: ConfirmationProgress,
}

impl ForkProgress {
    pub fn new(last_entry: Hash, prev_leader_slot: Option<Slot>, is_leader_slot: bool) -> Self {
        Self {
            is_dead: false,
            fork_stats: ForkStats::default(),
            replay_stats: ReplaySlotStats::default(),
            replay_progress: ConfirmationProgress::new(last_entry),
            propagated_stats: PropagatedStats {
                prev_leader_slot,
                is_leader_slot,
                ..PropagatedStats::default()
            },
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ForkStats {
    pub(crate) weight: u128,
    pub(crate) fork_weight: u128,
    pub(crate) total_staked: u64,
    pub(crate) slot: Slot,
    pub(crate) block_height: u64,
    pub(crate) has_voted: bool,
    pub(crate) is_recent: bool,
    pub(crate) is_empty: bool,
    pub(crate) vote_threshold: bool,
    pub(crate) is_locked_out: bool,
    pub(crate) stake_lockouts: HashMap<u64, StakeLockout>,
    pub(crate) confirmation_reported: bool,
    pub(crate) computed: bool,
}

#[derive(Clone, Default)]
pub(crate) struct PropagatedStats {
    pub(crate) propagated_validators: HashSet<Rc<Pubkey>>,
    pub(crate) propagated_validators_stake: u64,
    pub(crate) is_propagated: bool,
    pub(crate) is_leader_slot: bool,
    pub(crate) prev_leader_slot: Option<Slot>,
    pub(crate) slot_vote_tracker: Option<Arc<RwLock<SlotVoteTracker>>>,
}

#[derive(Default)]
pub(crate) struct ProgressMap {
    progress_map: HashMap<Slot, ForkProgress>,
}

impl std::ops::Deref for ProgressMap {
    type Target = HashMap<Slot, ForkProgress>;
    fn deref(&self) -> &Self::Target {
        &self.progress_map
    }
}

impl std::ops::DerefMut for ProgressMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.progress_map
    }
}

impl ProgressMap {
    pub fn insert(&mut self, slot: Slot, fork_progress: ForkProgress) {
        self.progress_map.insert(slot, fork_progress);
    }

    pub fn get_propagated_stats(&self, slot: Slot) -> Option<&PropagatedStats> {
        self.progress_map
            .get(&slot)
            .map(|fork_progress| &fork_progress.propagated_stats)
    }

    pub fn get_propagated_stats_mut(&mut self, slot: Slot) -> Option<&mut PropagatedStats> {
        self.progress_map
            .get_mut(&slot)
            .map(|fork_progress| &mut fork_progress.propagated_stats)
    }

    pub fn get_fork_stats(&self, slot: Slot) -> Option<&ForkStats> {
        self.progress_map
            .get(&slot)
            .map(|fork_progress| &fork_progress.fork_stats)
    }

    pub fn get_fork_stats_mut(&mut self, slot: Slot) -> Option<&mut ForkStats> {
        self.progress_map
            .get_mut(&slot)
            .map(|fork_progress| &mut fork_progress.fork_stats)
    }

    pub fn is_propagated(&mut self, slot: Slot) -> bool {
        // If the confirmation has already been seen, just returns
        let prev_leader_slot = {
            let propagated_stats = self
                .get_propagated_stats(slot)
                .expect("All frozen banks must exist in the Progress map");
            if propagated_stats.is_propagated {
                return true;
            }

            propagated_stats.prev_leader_slot
        };

        let prev_leader_confirmed =
            // prev_leader_slot doesn't exist because already rooted
            // or this validator hasn't been scheudled as a leader
            // yet. In both cases the latest leader is vacuously
            // confirmed
            prev_leader_slot.is_none() ||
                // prev_leader isn't in the progress map, which means
                // it's rooted, so it's confirmed
                self.get_propagated_stats(prev_leader_slot.unwrap()).is_none();

        if prev_leader_confirmed {
            // If previous leader has been confirmed as propagated, then
            // this block is also confirmed as propagated
            self.get_propagated_stats_mut(slot)
                .expect("All frozen banks must exist in the Progress map")
                .is_propagated = true;
            return true;
        }

        false
    }

    pub fn get_prev_leader_slot(&self, bank: &Bank) -> Option<Slot> {
        let parent_slot = bank.parent_slot();
        self.get_propagated_stats(parent_slot)
            .map(|stats| {
                if stats.is_leader_slot {
                    Some(parent_slot)
                } else {
                    stats.prev_leader_slot
                }
            })
            .unwrap_or(None)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_is_propagated() {
        let mut progress_map = ProgressMap::default();

        // Insert new ForkProgress for slot 10 and its
        // previous leader slot 9, and the previous leader
        // before that slot 8,
        progress_map.insert(10, ForkProgress::new(Hash::default(), Some(9), false));
        progress_map.insert(9, ForkProgress::new(Hash::default(), Some(8), true));
        progress_map.insert(8, ForkProgress::new(Hash::default(), Some(7), true));

        // Insert new ForkProgress with no previous leader
        progress_map.insert(3, ForkProgress::new(Hash::default(), None, true));

        // None of these slot have parents which are confirmed
        assert!(!progress_map.is_propagated(9));
        assert!(!progress_map.is_propagated(10));

        // The previous leader before 8, slot 7, does not exist in
        // progress map, so is_propagated(8) should return true as
        // this implies the parent is rooted
        assert!(progress_map.is_propagated(8));
        assert!(progress_map.get(&8).unwrap().propagated_stats.is_propagated);

        // Slot 3 has no previous leader slot, so the is_propagated() check
        // is vacuously true
        assert!(progress_map.is_propagated(3));
        assert!(progress_map.get(&3).unwrap().propagated_stats.is_propagated);

        // If we set the is_propagated = true, is_propagated should return true
        progress_map
            .get_mut(&9)
            .unwrap()
            .propagated_stats
            .is_propagated = true;
        assert!(progress_map.is_propagated(9), true);
        assert!(progress_map.get(&9).unwrap().propagated_stats.is_propagated);

        // Slot 10 is still unconfirmed, even though its previous leader slot 9
        // has been confirmed
        assert!(!progress_map.is_propagated(10), true);
    }
}

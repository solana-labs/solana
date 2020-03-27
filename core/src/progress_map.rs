use crate::{
    cluster_info_vote_listener::SlotVoteTracker, consensus::StakeLockout,
    replay_stage::SUPERMINORITY_THRESHOLD,
};
use solana_ledger::{
    bank_forks::BankForks,
    blockstore_processor::{ConfirmationProgress, ConfirmationTiming},
};
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

#[derive(Debug)]
pub(crate) struct ValidatorStakeInfo {
    pub validator_vote_pubkey: Pubkey,
    pub stake: u64,
    pub total_epoch_stake: u64,
}

impl Default for ValidatorStakeInfo {
    fn default() -> Self {
        Self {
            stake: 0,
            validator_vote_pubkey: Pubkey::default(),
            total_epoch_stake: 1,
        }
    }
}

impl ValidatorStakeInfo {
    pub fn new(validator_vote_pubkey: Pubkey, stake: u64, total_epoch_stake: u64) -> Self {
        Self {
            validator_vote_pubkey,
            stake,
            total_epoch_stake,
        }
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
    pub fn new(
        last_entry: Hash,
        prev_leader_slot: Option<Slot>,
        validator_stake_info: Option<ValidatorStakeInfo>,
    ) -> Self {
        let (
            is_leader_slot,
            propagated_validators_stake,
            propagated_validators,
            is_propagated,
            total_epoch_stake,
        ) = validator_stake_info
            .map(|info| {
                (
                    true,
                    info.stake,
                    vec![Rc::new(info.validator_vote_pubkey)]
                        .into_iter()
                        .collect(),
                    {
                        if info.total_epoch_stake == 0 {
                            true
                        } else {
                            info.stake as f64 / info.total_epoch_stake as f64
                                > SUPERMINORITY_THRESHOLD
                        }
                    },
                    info.total_epoch_stake,
                )
            })
            .unwrap_or((false, 0, HashSet::new(), false, 0));
        Self {
            is_dead: false,
            fork_stats: ForkStats::default(),
            replay_stats: ReplaySlotStats::default(),
            replay_progress: ConfirmationProgress::new(last_entry),
            propagated_stats: PropagatedStats {
                prev_leader_slot,
                is_leader_slot,
                propagated_validators_stake,
                propagated_validators,
                is_propagated,
                total_epoch_stake,
                ..PropagatedStats::default()
            },
        }
    }

    pub fn new_from_bank(
        bank: &Bank,
        my_pubkey: &Pubkey,
        voting_pubkey: &Pubkey,
        prev_leader_slot: Option<Slot>,
    ) -> Self {
        let validator_fork_info = {
            if bank.collector_id() == my_pubkey {
                let stake = bank.epoch_vote_account_stake(voting_pubkey);
                Some(ValidatorStakeInfo::new(
                    *voting_pubkey,
                    stake,
                    bank.total_epoch_stake(),
                ))
            } else {
                None
            }
        };

        Self::new(bank.last_blockhash(), prev_leader_slot, validator_fork_info)
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
    pub(crate) total_epoch_stake: u64,
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

    pub fn is_propagated(&self, slot: Slot) -> bool {
        let leader_slot_to_check = self.get_latest_leader_slot(slot);

        // prev_leader_slot doesn't exist because already rooted
        // or this validator hasn't been scheduled as a leader
        // yet. In both cases the latest leader is vacuously
        // confirmed
        leader_slot_to_check
            .map(|leader_slot_to_check| {
                // If the leader's stats are None (isn't in the
                // progress map), this means that prev_leader slot is
                // rooted, so return true
                self.get_propagated_stats(leader_slot_to_check)
                    .map(|stats| stats.is_propagated)
                    .unwrap_or(true)
            })
            .unwrap_or(true)
    }

    pub fn get_latest_leader_slot(&self, slot: Slot) -> Option<Slot> {
        let propagated_stats = self
            .get_propagated_stats(slot)
            .expect("All frozen banks must exist in the Progress map");

        if propagated_stats.is_leader_slot {
            Some(slot)
        } else {
            propagated_stats.prev_leader_slot
        }
    }

    pub fn get_bank_prev_leader_slot(&self, bank: &Bank) -> Option<Slot> {
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

    pub fn handle_new_root(&mut self, bank_forks: &BankForks) {
        self.progress_map
            .retain(|k, _| bank_forks.get(*k).is_some());
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_is_propagated_status_on_construction() {
        // If the given ValidatorStakeInfo == None, then this is not
        // a leader slot and is_propagated == false
        let progress = ForkProgress::new(Hash::default(), Some(9), None);
        assert!(!progress.propagated_stats.is_propagated);

        // If the stake is zero, then threshold is always achieved
        let progress = ForkProgress::new(
            Hash::default(),
            Some(9),
            Some(ValidatorStakeInfo {
                total_epoch_stake: 0,
                ..ValidatorStakeInfo::default()
            }),
        );
        assert!(progress.propagated_stats.is_propagated);

        // If the stake is non zero, then threshold is not achieved unless
        // validator has enough stake by itself to pass threshold
        let progress = ForkProgress::new(
            Hash::default(),
            Some(9),
            Some(ValidatorStakeInfo {
                total_epoch_stake: 2,
                ..ValidatorStakeInfo::default()
            }),
        );
        assert!(!progress.propagated_stats.is_propagated);

        // Give the validator enough stake by itself to pass threshold
        let progress = ForkProgress::new(
            Hash::default(),
            Some(9),
            Some(ValidatorStakeInfo {
                stake: 1,
                total_epoch_stake: 2,
                ..ValidatorStakeInfo::default()
            }),
        );
        assert!(progress.propagated_stats.is_propagated);

        // Check that the default ValidatorStakeInfo::default() constructs a ForkProgress
        // with is_propagated == false, otherwise propagation tests will fail to run
        // the proper checks (most will auto-pass without checking anything)
        let progress = ForkProgress::new(
            Hash::default(),
            Some(9),
            Some(ValidatorStakeInfo::default()),
        );
        assert!(!progress.propagated_stats.is_propagated);
    }

    #[test]
    fn test_is_propagated() {
        let mut progress_map = ProgressMap::default();

        // Insert new ForkProgress for slot 10 (not a leader slot) and its
        // previous leader slot 9 (leader slot)
        progress_map.insert(10, ForkProgress::new(Hash::default(), Some(9), None));
        progress_map.insert(
            9,
            ForkProgress::new(Hash::default(), None, Some(ValidatorStakeInfo::default())),
        );

        // None of these slot have parents which are confirmed
        assert!(!progress_map.is_propagated(9));
        assert!(!progress_map.is_propagated(10));

        // Insert new ForkProgress for slot 8 with no previous leader.
        // The previous leader before 8, slot 7, does not exist in
        // progress map, so is_propagated(8) should return true as
        // this implies the parent is rooted
        progress_map.insert(8, ForkProgress::new(Hash::default(), Some(7), None));
        assert!(progress_map.is_propagated(8));

        // If we set the is_propagated = true, is_propagated should return true
        progress_map
            .get_propagated_stats_mut(9)
            .unwrap()
            .is_propagated = true;
        assert!(progress_map.is_propagated(9));
        assert!(progress_map.get(&9).unwrap().propagated_stats.is_propagated);

        // Because slot 9 is now confirmed, then slot 10 is also confirmed b/c 9
        // is the last leader slot before 10
        assert!(progress_map.is_propagated(10));

        // If we make slot 10 a leader slot though, even though its previous
        // leader slot 9 has been confirmed, slot 10 itself is not confirmed
        progress_map
            .get_propagated_stats_mut(10)
            .unwrap()
            .is_leader_slot = true;
        assert!(!progress_map.is_propagated(10));
    }
}

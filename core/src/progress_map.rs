use crate::{
    cluster_info_vote_listener::SlotVoteTracker,
    cluster_slots::SlotPubkeys,
    replay_stage::SUPERMINORITY_THRESHOLD,
    {consensus::Stake, consensus::VotedStakes},
};
use solana_ledger::blockstore_processor::{ConfirmationProgress, ConfirmationTiming};
use solana_runtime::{bank::Bank, bank_forks::BankForks, vote_account::ArcVoteAccount};
use solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, RwLock},
};

type VotedSlot = Slot;
type ExpirationSlot = Slot;
pub(crate) type LockoutIntervals = BTreeMap<ExpirationSlot, Vec<(VotedSlot, Pubkey)>>;

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
            (
                "entry_poh_verification_time",
                self.poh_verify_elapsed as i64,
                i64
            ),
            (
                "entry_transaction_verification_time",
                self.transaction_verify_elapsed as i64,
                i64
            ),
            ("replay_time", self.replay_elapsed as i64, i64),
            (
                "replay_total_elapsed",
                self.started.elapsed().as_micros() as i64,
                i64
            ),
            ("total_entries", num_entries as i64, i64),
            ("total_shreds", num_shreds as i64, i64),
            ("check_us", self.execute_timings.check_us, i64),
            ("load_us", self.execute_timings.load_us, i64),
            ("execute_us", self.execute_timings.execute_us, i64),
            ("store_us", self.execute_timings.store_us, i64),
            (
                "serialize_us",
                self.execute_timings.details.serialize_us,
                i64
            ),
            (
                "create_vm_us",
                self.execute_timings.details.create_vm_us,
                i64
            ),
            (
                "execute_inner_us",
                self.execute_timings.details.execute_us,
                i64
            ),
            (
                "deserialize_us",
                self.execute_timings.details.deserialize_us,
                i64
            ),
            (
                "changed_account_count",
                self.execute_timings.details.changed_account_count,
                i64
            ),
            (
                "total_account_count",
                self.execute_timings.details.total_account_count,
                i64
            ),
            (
                "total_data_size",
                self.execute_timings.details.total_data_size,
                i64
            ),
            (
                "data_size_changed",
                self.execute_timings.details.data_size_changed,
                i64
            ),
        );

        let mut per_pubkey_timings: Vec<_> = self
            .execute_timings
            .details
            .per_program_timings
            .iter()
            .collect();
        per_pubkey_timings.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        let total: u64 = per_pubkey_timings.iter().map(|a| a.1 .0).sum();
        for (pubkey, time) in per_pubkey_timings.iter().take(5) {
            datapoint_info!(
                "per_program_timings",
                ("pubkey", pubkey.to_string(), String),
                ("execute_us", time.0, i64)
            );
        }
        datapoint_info!(
            "per_program_timings",
            ("pubkey", "all", String),
            ("execute_us", total, i64)
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
    pub(crate) duplicate_stats: DuplicateStats,
    // Note `num_blocks_on_fork` and `num_dropped_blocks_on_fork` only
    // count new blocks replayed since last restart, which won't include
    // blocks already existing in the ledger/before snapshot at start,
    // so these stats do not span all of time
    pub(crate) num_blocks_on_fork: u64,
    pub(crate) num_dropped_blocks_on_fork: u64,
}

impl ForkProgress {
    pub fn new(
        last_entry: Hash,
        prev_leader_slot: Option<Slot>,
        duplicate_stats: DuplicateStats,
        validator_stake_info: Option<ValidatorStakeInfo>,
        num_blocks_on_fork: u64,
        num_dropped_blocks_on_fork: u64,
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
                    vec![info.validator_vote_pubkey].into_iter().collect(),
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
            duplicate_stats,
            num_blocks_on_fork,
            num_dropped_blocks_on_fork,
            propagated_stats: PropagatedStats {
                propagated_validators,
                propagated_validators_stake,
                is_propagated,
                is_leader_slot,
                prev_leader_slot,
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
        duplicate_stats: DuplicateStats,
        num_blocks_on_fork: u64,
        num_dropped_blocks_on_fork: u64,
    ) -> Self {
        let validator_stake_info = {
            if bank.collector_id() == my_pubkey {
                Some(ValidatorStakeInfo::new(
                    *voting_pubkey,
                    bank.epoch_vote_account_stake(voting_pubkey),
                    bank.total_epoch_stake(),
                ))
            } else {
                None
            }
        };

        Self::new(
            bank.last_blockhash(),
            prev_leader_slot,
            duplicate_stats,
            validator_stake_info,
            num_blocks_on_fork,
            num_dropped_blocks_on_fork,
        )
    }

    pub fn is_duplicate_confirmed(&self) -> bool {
        self.duplicate_stats.is_duplicate_confirmed
    }

    pub fn set_duplicate_confirmed(&mut self) {
        self.duplicate_stats.set_duplicate_confirmed();
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ForkStats {
    pub(crate) weight: u128,
    pub(crate) fork_weight: u128,
    pub(crate) total_stake: Stake,
    pub(crate) block_height: u64,
    pub(crate) has_voted: bool,
    pub(crate) is_recent: bool,
    pub(crate) is_empty: bool,
    pub(crate) vote_threshold: bool,
    pub(crate) is_locked_out: bool,
    pub(crate) voted_stakes: VotedStakes,
    pub(crate) is_supermajority_confirmed: bool,
    pub(crate) computed: bool,
    pub(crate) lockout_intervals: LockoutIntervals,
    pub(crate) bank_hash: Option<Hash>,
    pub(crate) my_latest_landed_vote: Option<Slot>,
}

#[derive(Clone, Default)]
pub(crate) struct PropagatedStats {
    pub(crate) propagated_validators: HashSet<Pubkey>,
    pub(crate) propagated_node_ids: HashSet<Pubkey>,
    pub(crate) propagated_validators_stake: u64,
    pub(crate) is_propagated: bool,
    pub(crate) is_leader_slot: bool,
    pub(crate) prev_leader_slot: Option<Slot>,
    pub(crate) slot_vote_tracker: Option<Arc<RwLock<SlotVoteTracker>>>,
    pub(crate) cluster_slot_pubkeys: Option<Arc<RwLock<SlotPubkeys>>>,
    pub(crate) total_epoch_stake: u64,
}

#[derive(Clone, Default)]
pub(crate) struct DuplicateStats {
    latest_unconfirmed_duplicate_ancestor: Option<Slot>,
    is_duplicate_confirmed: bool,
}

impl DuplicateStats {
    pub fn new_with_unconfirmed_duplicate_ancestor(
        latest_unconfirmed_duplicate_ancestor: Option<Slot>,
    ) -> Self {
        Self {
            latest_unconfirmed_duplicate_ancestor,
            is_duplicate_confirmed: false,
        }
    }

    fn set_duplicate_confirmed(&mut self) {
        self.is_duplicate_confirmed = true;
        self.latest_unconfirmed_duplicate_ancestor = None;
    }

    fn update_with_newly_confirmed_duplicate_ancestor(&mut self, newly_confirmed_ancestor: Slot) {
        if let Some(latest_unconfirmed_duplicate_ancestor) =
            self.latest_unconfirmed_duplicate_ancestor
        {
            if latest_unconfirmed_duplicate_ancestor <= newly_confirmed_ancestor {
                self.latest_unconfirmed_duplicate_ancestor = None;
            }
        }
    }
}

impl PropagatedStats {
    pub fn add_vote_pubkey(&mut self, vote_pubkey: Pubkey, stake: u64) {
        if self.propagated_validators.insert(vote_pubkey) {
            self.propagated_validators_stake += stake;
        }
    }

    pub fn add_node_pubkey(&mut self, node_pubkey: &Pubkey, bank: &Bank) {
        if !self.propagated_node_ids.contains(node_pubkey) {
            let node_vote_accounts = bank
                .epoch_vote_accounts_for_node_id(&node_pubkey)
                .map(|v| &v.vote_accounts);

            if let Some(node_vote_accounts) = node_vote_accounts {
                self.add_node_pubkey_internal(
                    node_pubkey,
                    node_vote_accounts,
                    bank.epoch_vote_accounts(bank.epoch())
                        .expect("Epoch stakes for bank's own epoch must exist"),
                );
            }
        }
    }

    fn add_node_pubkey_internal(
        &mut self,
        node_pubkey: &Pubkey,
        vote_account_pubkeys: &[Pubkey],
        epoch_vote_accounts: &HashMap<Pubkey, (u64, ArcVoteAccount)>,
    ) {
        self.propagated_node_ids.insert(*node_pubkey);
        for vote_account_pubkey in vote_account_pubkeys.iter() {
            let stake = epoch_vote_accounts
                .get(vote_account_pubkey)
                .map(|(stake, _)| *stake)
                .unwrap_or(0);
            self.add_vote_pubkey(*vote_account_pubkey, stake);
        }
    }
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

    pub fn is_dead(&self, slot: Slot) -> Option<bool> {
        self.progress_map
            .get(&slot)
            .map(|fork_progress| fork_progress.is_dead)
    }

    pub fn get_hash(&self, slot: Slot) -> Option<Hash> {
        self.progress_map
            .get(&slot)
            .and_then(|fork_progress| fork_progress.fork_stats.bank_hash)
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

    #[cfg(test)]
    pub fn is_unconfirmed_duplicate(&self, slot: Slot) -> Option<bool> {
        self.get(&slot).map(|p| {
            p.duplicate_stats
                .latest_unconfirmed_duplicate_ancestor
                .map(|ancestor| ancestor == slot)
                .unwrap_or(false)
        })
    }

    pub fn latest_unconfirmed_duplicate_ancestor(&self, slot: Slot) -> Option<Slot> {
        self.get(&slot)
            .map(|p| p.duplicate_stats.latest_unconfirmed_duplicate_ancestor)
            .unwrap_or(None)
    }

    pub fn set_unconfirmed_duplicate_slot(&mut self, slot: Slot, descendants: &HashSet<u64>) {
        if let Some(fork_progress) = self.get_mut(&slot) {
            if fork_progress.is_duplicate_confirmed() {
                assert!(fork_progress
                    .duplicate_stats
                    .latest_unconfirmed_duplicate_ancestor
                    .is_none());
                return;
            }

            if fork_progress
                .duplicate_stats
                .latest_unconfirmed_duplicate_ancestor
                == Some(slot)
            {
                // Already been marked
                return;
            }
            fork_progress
                .duplicate_stats
                .latest_unconfirmed_duplicate_ancestor = Some(slot);

            for d in descendants {
                if let Some(fork_progress) = self.get_mut(&d) {
                    fork_progress
                        .duplicate_stats
                        .latest_unconfirmed_duplicate_ancestor = Some(std::cmp::max(
                        fork_progress
                            .duplicate_stats
                            .latest_unconfirmed_duplicate_ancestor
                            .unwrap_or(0),
                        slot,
                    ));
                }
            }
        }
    }

    pub fn set_confirmed_duplicate_slot(
        &mut self,
        slot: Slot,
        ancestors: &HashSet<u64>,
        descendants: &HashSet<u64>,
    ) {
        for a in ancestors {
            if let Some(fork_progress) = self.get_mut(&a) {
                fork_progress.set_duplicate_confirmed();
            }
        }

        if let Some(slot_fork_progress) = self.get_mut(&slot) {
            // Setting the fields here is nly correct and necessary if the loop above didn't
            // already do this, so check with an assert.
            assert!(!ancestors.contains(&slot));
            let slot_had_unconfirmed_duplicate_ancestor = slot_fork_progress
                .duplicate_stats
                .latest_unconfirmed_duplicate_ancestor
                .is_some();
            slot_fork_progress.set_duplicate_confirmed();

            if slot_had_unconfirmed_duplicate_ancestor {
                for d in descendants {
                    if let Some(descendant_fork_progress) = self.get_mut(&d) {
                        descendant_fork_progress
                            .duplicate_stats
                            .update_with_newly_confirmed_duplicate_ancestor(slot);
                    }
                }
            } else {
                // Neither this slot `S`, nor earlier ancestors were marked as duplicate,
                // so this means all descendants either:
                // 1) Have no duplicate ancestors
                // 2) Have a duplicate ancestor > `S`

                // In both cases, there's no need to iterate through descendants because
                // this confirmation on `S` is irrelevant to them.
            }
        }
    }

    pub fn my_latest_landed_vote(&self, slot: Slot) -> Option<Slot> {
        self.progress_map
            .get(&slot)
            .and_then(|s| s.fork_stats.my_latest_landed_vote)
    }

    pub fn set_supermajority_confirmed_slot(&mut self, slot: Slot) {
        let slot_progress = self.get_mut(&slot).unwrap();
        slot_progress.fork_stats.is_supermajority_confirmed = true;
    }

    pub fn is_supermajority_confirmed(&self, slot: Slot) -> Option<bool> {
        self.progress_map
            .get(&slot)
            .map(|s| s.fork_stats.is_supermajority_confirmed)
    }

    pub fn is_duplicate_confirmed(&self, slot: Slot) -> Option<bool> {
        self.progress_map
            .get(&slot)
            .map(|s| s.is_duplicate_confirmed())
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

    pub fn log_propagated_stats(&self, slot: Slot, bank_forks: &RwLock<BankForks>) {
        if let Some(stats) = self.get_propagated_stats(slot) {
            info!(
                "Propagated stats:
                total staked: {},
                observed staked: {},
                vote pubkeys: {:?},
                node_pubkeys: {:?},
                slot: {},
                epoch: {:?}",
                stats.total_epoch_stake,
                stats.propagated_validators_stake,
                stats.propagated_validators,
                stats.propagated_node_ids,
                slot,
                bank_forks.read().unwrap().get(slot).map(|x| x.epoch()),
            );
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::consensus::test::VoteSimulator;
    use trees::tr;

    #[test]
    fn test_add_vote_pubkey() {
        let mut stats = PropagatedStats::default();
        let mut vote_pubkey = solana_sdk::pubkey::new_rand();

        // Add a vote pubkey, the number of references in all_pubkeys
        // should be 2
        stats.add_vote_pubkey(vote_pubkey, 1);
        assert!(stats.propagated_validators.contains(&vote_pubkey));
        assert_eq!(stats.propagated_validators_stake, 1);

        // Adding it again should change no state since the key already existed
        stats.add_vote_pubkey(vote_pubkey, 1);
        assert!(stats.propagated_validators.contains(&vote_pubkey));
        assert_eq!(stats.propagated_validators_stake, 1);

        // Adding another pubkey should succeed
        vote_pubkey = solana_sdk::pubkey::new_rand();
        stats.add_vote_pubkey(vote_pubkey, 2);
        assert!(stats.propagated_validators.contains(&vote_pubkey));
        assert_eq!(stats.propagated_validators_stake, 3);
    }

    #[test]
    fn test_add_node_pubkey_internal() {
        let num_vote_accounts = 10;
        let staked_vote_accounts = 5;
        let vote_account_pubkeys: Vec<_> = std::iter::repeat_with(solana_sdk::pubkey::new_rand)
            .take(num_vote_accounts)
            .collect();
        let epoch_vote_accounts: HashMap<_, _> = vote_account_pubkeys
            .iter()
            .skip(num_vote_accounts - staked_vote_accounts)
            .map(|pubkey| (*pubkey, (1, ArcVoteAccount::default())))
            .collect();

        let mut stats = PropagatedStats::default();
        let mut node_pubkey = solana_sdk::pubkey::new_rand();

        // Add a vote pubkey, the number of references in all_pubkeys
        // should be 2
        stats.add_node_pubkey_internal(&node_pubkey, &vote_account_pubkeys, &epoch_vote_accounts);
        assert!(stats.propagated_node_ids.contains(&node_pubkey));
        assert_eq!(
            stats.propagated_validators_stake,
            staked_vote_accounts as u64
        );

        // Adding it again should not change any state
        stats.add_node_pubkey_internal(&node_pubkey, &vote_account_pubkeys, &epoch_vote_accounts);
        assert!(stats.propagated_node_ids.contains(&node_pubkey));
        assert_eq!(
            stats.propagated_validators_stake,
            staked_vote_accounts as u64
        );

        // Adding another pubkey with same vote accounts should succeed, but stake
        // shouldn't increase
        node_pubkey = solana_sdk::pubkey::new_rand();
        stats.add_node_pubkey_internal(&node_pubkey, &vote_account_pubkeys, &epoch_vote_accounts);
        assert!(stats.propagated_node_ids.contains(&node_pubkey));
        assert_eq!(
            stats.propagated_validators_stake,
            staked_vote_accounts as u64
        );

        // Adding another pubkey with different vote accounts should succeed
        // and increase stake
        node_pubkey = solana_sdk::pubkey::new_rand();
        let vote_account_pubkeys: Vec<_> = std::iter::repeat_with(solana_sdk::pubkey::new_rand)
            .take(num_vote_accounts)
            .collect();
        let epoch_vote_accounts: HashMap<_, _> = vote_account_pubkeys
            .iter()
            .skip(num_vote_accounts - staked_vote_accounts)
            .map(|pubkey| (*pubkey, (1, ArcVoteAccount::default())))
            .collect();
        stats.add_node_pubkey_internal(&node_pubkey, &vote_account_pubkeys, &epoch_vote_accounts);
        assert!(stats.propagated_node_ids.contains(&node_pubkey));
        assert_eq!(
            stats.propagated_validators_stake,
            2 * staked_vote_accounts as u64
        );
    }

    #[test]
    fn test_is_propagated_status_on_construction() {
        // If the given ValidatorStakeInfo == None, then this is not
        // a leader slot and is_propagated == false
        let progress = ForkProgress::new(
            Hash::default(),
            Some(9),
            DuplicateStats::default(),
            None,
            0,
            0,
        );
        assert!(!progress.propagated_stats.is_propagated);

        // If the stake is zero, then threshold is always achieved
        let progress = ForkProgress::new(
            Hash::default(),
            Some(9),
            DuplicateStats::default(),
            Some(ValidatorStakeInfo {
                total_epoch_stake: 0,
                ..ValidatorStakeInfo::default()
            }),
            0,
            0,
        );
        assert!(progress.propagated_stats.is_propagated);

        // If the stake is non zero, then threshold is not achieved unless
        // validator has enough stake by itself to pass threshold
        let progress = ForkProgress::new(
            Hash::default(),
            Some(9),
            DuplicateStats::default(),
            Some(ValidatorStakeInfo {
                total_epoch_stake: 2,
                ..ValidatorStakeInfo::default()
            }),
            0,
            0,
        );
        assert!(!progress.propagated_stats.is_propagated);

        // Give the validator enough stake by itself to pass threshold
        let progress = ForkProgress::new(
            Hash::default(),
            Some(9),
            DuplicateStats::default(),
            Some(ValidatorStakeInfo {
                stake: 1,
                total_epoch_stake: 2,
                ..ValidatorStakeInfo::default()
            }),
            0,
            0,
        );
        assert!(progress.propagated_stats.is_propagated);

        // Check that the default ValidatorStakeInfo::default() constructs a ForkProgress
        // with is_propagated == false, otherwise propagation tests will fail to run
        // the proper checks (most will auto-pass without checking anything)
        let progress = ForkProgress::new(
            Hash::default(),
            Some(9),
            DuplicateStats::default(),
            Some(ValidatorStakeInfo::default()),
            0,
            0,
        );
        assert!(!progress.propagated_stats.is_propagated);
    }

    #[test]
    fn test_is_propagated() {
        let mut progress_map = ProgressMap::default();

        // Insert new ForkProgress for slot 10 (not a leader slot) and its
        // previous leader slot 9 (leader slot)
        progress_map.insert(
            10,
            ForkProgress::new(
                Hash::default(),
                Some(9),
                DuplicateStats::default(),
                None,
                0,
                0,
            ),
        );
        progress_map.insert(
            9,
            ForkProgress::new(
                Hash::default(),
                None,
                DuplicateStats::default(),
                Some(ValidatorStakeInfo::default()),
                0,
                0,
            ),
        );

        // None of these slot have parents which are confirmed
        assert!(!progress_map.is_propagated(9));
        assert!(!progress_map.is_propagated(10));

        // Insert new ForkProgress for slot 8 with no previous leader.
        // The previous leader before 8, slot 7, does not exist in
        // progress map, so is_propagated(8) should return true as
        // this implies the parent is rooted
        progress_map.insert(
            8,
            ForkProgress::new(
                Hash::default(),
                Some(7),
                DuplicateStats::default(),
                None,
                0,
                0,
            ),
        );
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

    fn setup_set_unconfirmed_and_confirmed_duplicate_slot_tests(
        smaller_duplicate_slot: Slot,
        larger_duplicate_slot: Slot,
    ) -> (ProgressMap, RwLock<BankForks>) {
        // Create simple fork 0 -> 1 -> 2 -> 3 -> 4 -> 5
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(3) / (tr(4) / tr(5)))));
        let mut vote_simulator = VoteSimulator::new(1);
        vote_simulator.fill_bank_forks(forks, &HashMap::new());
        let VoteSimulator {
            mut progress,
            bank_forks,
            ..
        } = vote_simulator;
        let descendants = bank_forks.read().unwrap().descendants().clone();

        // Mark the slots as unconfirmed duplicates
        progress.set_unconfirmed_duplicate_slot(
            smaller_duplicate_slot,
            &descendants.get(&smaller_duplicate_slot).unwrap(),
        );
        progress.set_unconfirmed_duplicate_slot(
            larger_duplicate_slot,
            &descendants.get(&larger_duplicate_slot).unwrap(),
        );

        // Correctness checks
        for slot in bank_forks.read().unwrap().banks().keys() {
            if *slot < smaller_duplicate_slot {
                assert!(progress
                    .latest_unconfirmed_duplicate_ancestor(*slot)
                    .is_none());
            } else if *slot < larger_duplicate_slot {
                assert_eq!(
                    progress
                        .latest_unconfirmed_duplicate_ancestor(*slot)
                        .unwrap(),
                    smaller_duplicate_slot
                );
            } else {
                assert_eq!(
                    progress
                        .latest_unconfirmed_duplicate_ancestor(*slot)
                        .unwrap(),
                    larger_duplicate_slot
                );
            }
        }

        (progress, bank_forks)
    }

    #[test]
    fn test_set_unconfirmed_duplicate_confirm_smaller_slot_first() {
        let smaller_duplicate_slot = 1;
        let larger_duplicate_slot = 4;
        let (mut progress, bank_forks) = setup_set_unconfirmed_and_confirmed_duplicate_slot_tests(
            smaller_duplicate_slot,
            larger_duplicate_slot,
        );
        let descendants = bank_forks.read().unwrap().descendants().clone();
        let ancestors = bank_forks.read().unwrap().ancestors();

        // Mark the smaller duplicate slot as confirmed
        progress.set_confirmed_duplicate_slot(
            smaller_duplicate_slot,
            &ancestors.get(&smaller_duplicate_slot).unwrap(),
            &descendants.get(&smaller_duplicate_slot).unwrap(),
        );
        for slot in bank_forks.read().unwrap().banks().keys() {
            if *slot < larger_duplicate_slot {
                // Only slots <= smaller_duplicate_slot have been duplicate confirmed
                if *slot <= smaller_duplicate_slot {
                    assert!(progress.is_duplicate_confirmed(*slot).unwrap());
                } else {
                    assert!(!progress.is_duplicate_confirmed(*slot).unwrap());
                }
                // The unconfirmed duplicate flag has been cleared on the smaller
                // descendants because their most recent duplicate ancestor has
                // been confirmed
                assert!(progress
                    .latest_unconfirmed_duplicate_ancestor(*slot)
                    .is_none());
            } else {
                assert!(!progress.is_duplicate_confirmed(*slot).unwrap(),);
                // The unconfirmed duplicate flag has not been cleared on the smaller
                // descendants because their most recent duplicate ancestor,
                // `larger_duplicate_slot` has  not yet been confirmed
                assert_eq!(
                    progress
                        .latest_unconfirmed_duplicate_ancestor(*slot)
                        .unwrap(),
                    larger_duplicate_slot
                );
            }
        }

        // Mark the larger duplicate slot as confirmed, all slots should no longer
        // have any unconfirmed duplicate ancestors, and should be marked as duplciate confirmed
        progress.set_confirmed_duplicate_slot(
            larger_duplicate_slot,
            &ancestors.get(&larger_duplicate_slot).unwrap(),
            &descendants.get(&larger_duplicate_slot).unwrap(),
        );
        for slot in bank_forks.read().unwrap().banks().keys() {
            // All slots <= the latest duplciate confirmed slot are ancestors of
            // that slot, so they should all be marked duplicate confirmed
            assert_eq!(
                progress.is_duplicate_confirmed(*slot).unwrap(),
                *slot <= larger_duplicate_slot
            );
            assert!(progress
                .latest_unconfirmed_duplicate_ancestor(*slot)
                .is_none());
        }
    }

    #[test]
    fn test_set_unconfirmed_duplicate_confirm_larger_slot_first() {
        let smaller_duplicate_slot = 1;
        let larger_duplicate_slot = 4;
        let (mut progress, bank_forks) = setup_set_unconfirmed_and_confirmed_duplicate_slot_tests(
            smaller_duplicate_slot,
            larger_duplicate_slot,
        );
        let descendants = bank_forks.read().unwrap().descendants().clone();
        let ancestors = bank_forks.read().unwrap().ancestors();

        // Mark the larger duplicate slot as confirmed
        progress.set_confirmed_duplicate_slot(
            larger_duplicate_slot,
            &ancestors.get(&larger_duplicate_slot).unwrap(),
            &descendants.get(&larger_duplicate_slot).unwrap(),
        );

        // All slots should no longer have any unconfirmed duplicate ancestors
        progress.set_confirmed_duplicate_slot(
            larger_duplicate_slot,
            &ancestors.get(&larger_duplicate_slot).unwrap(),
            &descendants.get(&larger_duplicate_slot).unwrap(),
        );
        for slot in bank_forks.read().unwrap().banks().keys() {
            // All slots <= the latest duplciate confirmed slot are ancestors of
            // that slot, so they should all be marked duplicate confirmed
            assert_eq!(
                progress.is_duplicate_confirmed(*slot).unwrap(),
                *slot <= larger_duplicate_slot
            );
            assert!(progress
                .latest_unconfirmed_duplicate_ancestor(*slot)
                .is_none());
        }
    }
}

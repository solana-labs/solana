use crate::bank_forks::BankForks;
use crate::staking_utils;
use hashbrown::{HashMap, HashSet};
use solana_metrics::datapoint;
use solana_runtime::bank::Bank;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_vote_api::vote_state::{Lockout, Vote, VoteState, MAX_LOCKOUT_HISTORY};
use std::sync::Arc;

pub const VOTE_THRESHOLD_DEPTH: usize = 8;
pub const VOTE_THRESHOLD_SIZE: f64 = 2f64 / 3f64;
const MAX_RECENT_VOTES: usize = 16;

#[derive(Default)]
pub struct EpochStakes {
    epoch: u64,
    stakes: HashMap<Pubkey, u64>,
    self_staked: u64,
    total_staked: u64,
    delegate_id: Pubkey,
}

#[derive(Default, Debug)]
pub struct StakeLockout {
    lockout: u64,
    stake: u64,
}

#[derive(Default)]
pub struct Locktower {
    epoch_stakes: EpochStakes,
    threshold_depth: usize,
    threshold_size: f64,
    lockouts: VoteState,
}

impl EpochStakes {
    pub fn new(epoch: u64, stakes: HashMap<Pubkey, u64>, delegate_id: &Pubkey) -> Self {
        let total_staked = stakes.values().sum();
        let self_staked = *stakes.get(&delegate_id).unwrap_or(&0);
        Self {
            epoch,
            stakes,
            total_staked,
            self_staked,
            delegate_id: *delegate_id,
        }
    }
    pub fn new_for_tests(lamports: u64) -> Self {
        Self::new(
            0,
            vec![(Pubkey::default(), lamports)].into_iter().collect(),
            &Pubkey::default(),
        )
    }
    pub fn new_from_stakes(epoch: u64, accounts: &[(Pubkey, (u64, Account))]) -> Self {
        let stakes = accounts.iter().map(|(k, (v, _))| (*k, *v)).collect();
        Self::new(epoch, stakes, &accounts[0].0)
    }
    pub fn new_from_bank(bank: &Bank, my_id: &Pubkey) -> Self {
        let bank_epoch = bank.get_epoch_and_slot_index(bank.slot()).0;
        let stakes = staking_utils::vote_account_stakes_at_epoch(bank, bank_epoch)
            .expect("voting require a bank with stakes");
        Self::new(bank_epoch, stakes, my_id)
    }
}

impl Locktower {
    pub fn new_from_forks(bank_forks: &BankForks, my_id: &Pubkey) -> Self {
        let mut frozen_banks: Vec<_> = bank_forks.frozen_banks().values().cloned().collect();
        frozen_banks.sort_by_key(|b| (b.parents().len(), b.slot()));
        let epoch_stakes = {
            if let Some(bank) = frozen_banks.last() {
                EpochStakes::new_from_bank(bank, my_id)
            } else {
                return Self::default();
            }
        };

        let mut locktower = Self {
            epoch_stakes,
            threshold_depth: VOTE_THRESHOLD_DEPTH,
            threshold_size: VOTE_THRESHOLD_SIZE,
            lockouts: VoteState::default(),
        };

        let bank = locktower.find_heaviest_bank(bank_forks).unwrap();
        locktower.lockouts =
            Self::initialize_lockouts_from_bank(&bank, locktower.epoch_stakes.epoch);
        locktower
    }
    pub fn new(epoch_stakes: EpochStakes, threshold_depth: usize, threshold_size: f64) -> Self {
        Self {
            epoch_stakes,
            threshold_depth,
            threshold_size,
            lockouts: VoteState::default(),
        }
    }
    pub fn collect_vote_lockouts<F>(
        &self,
        bank_slot: u64,
        vote_accounts: F,
        ancestors: &HashMap<u64, HashSet<u64>>,
    ) -> HashMap<u64, StakeLockout>
    where
        F: Iterator<Item = (Pubkey, (u64, Account))>,
    {
        let mut stake_lockouts = HashMap::new();
        for (key, (_, account)) in vote_accounts {
            let lamports: u64 = *self.epoch_stakes.stakes.get(&key).unwrap_or(&0);
            if lamports == 0 {
                continue;
            }
            let mut vote_state: VoteState = VoteState::deserialize(&account.data)
                .expect("bank should always have valid VoteState data");

            if key == self.epoch_stakes.delegate_id
                || vote_state.node_id == self.epoch_stakes.delegate_id
            {
                debug!("vote state {:?}", vote_state);
                debug!(
                    "observed slot {}",
                    vote_state.nth_recent_vote(0).map(|v| v.slot).unwrap_or(0) as i64
                );
                debug!("observed root {}", vote_state.root_slot.unwrap_or(0) as i64);
                datapoint!(
                    "locktower-observed",
                    (
                        "slot",
                        vote_state.nth_recent_vote(0).map(|v| v.slot).unwrap_or(0),
                        i64
                    ),
                    ("root", vote_state.root_slot.unwrap_or(0), i64)
                );
            }
            let start_root = vote_state.root_slot;
            vote_state.process_vote(&Vote { slot: bank_slot });
            for vote in &vote_state.votes {
                Self::update_ancestor_lockouts(&mut stake_lockouts, &vote, ancestors);
            }
            if start_root != vote_state.root_slot {
                if let Some(root) = start_root {
                    let vote = Lockout {
                        confirmation_count: MAX_LOCKOUT_HISTORY as u32,
                        slot: root,
                    };
                    trace!("ROOT: {}", vote.slot);
                    Self::update_ancestor_lockouts(&mut stake_lockouts, &vote, ancestors);
                }
            }
            if let Some(root) = vote_state.root_slot {
                let vote = Lockout {
                    confirmation_count: MAX_LOCKOUT_HISTORY as u32,
                    slot: root,
                };
                Self::update_ancestor_lockouts(&mut stake_lockouts, &vote, ancestors);
            }

            // The last vote in the vote stack is a simulated vote on bank_slot, which
            // we added to the vote stack earlier in this function by calling process_vote().
            // We don't want to update the ancestors stakes of this vote b/c it does not
            // represent an actual vote by the validator.

            // Note: It should not be possible for any vote state in this bank to have
            // a vote for a slot >= bank_slot, so we are guaranteed that the last vote in
            // this vote stack is the simulated vote, so this fetch should be sufficient
            // to find the last unsimulated vote.
            assert_eq!(
                vote_state.nth_recent_vote(0).map(|l| l.slot),
                Some(bank_slot)
            );
            if let Some(vote) = vote_state.nth_recent_vote(1) {
                // Update all the parents of this last vote with the stake of this vote account
                Self::update_ancestor_stakes(&mut stake_lockouts, vote.slot, lamports, ancestors);
            }
        }
        stake_lockouts
    }

    pub fn is_slot_confirmed(&self, slot: u64, lockouts: &HashMap<u64, StakeLockout>) -> bool {
        lockouts
            .get(&slot)
            .map(|lockout| {
                (lockout.stake as f64 / self.epoch_stakes.total_staked as f64) > self.threshold_size
            })
            .unwrap_or(false)
    }

    pub fn is_recent_epoch(&self, bank: &Bank) -> bool {
        let bank_epoch = bank.get_epoch_and_slot_index(bank.slot()).0;
        bank_epoch >= self.epoch_stakes.epoch
    }

    pub fn update_epoch(&mut self, bank: &Bank) {
        trace!(
            "updating bank epoch slot: {} epoch: {}",
            bank.slot(),
            self.epoch_stakes.epoch
        );
        let bank_epoch = bank.get_epoch_and_slot_index(bank.slot()).0;
        if bank_epoch != self.epoch_stakes.epoch {
            assert!(
                self.is_recent_epoch(bank),
                "epoch_stakes cannot move backwards"
            );
            info!(
                "Locktower updated epoch bank slot: {} epoch: {}",
                bank.slot(),
                self.epoch_stakes.epoch
            );
            self.epoch_stakes = EpochStakes::new_from_bank(bank, &self.epoch_stakes.delegate_id);
            datapoint!(
                "locktower-epoch",
                ("epoch", self.epoch_stakes.epoch, i64),
                ("self_staked", self.epoch_stakes.self_staked, i64),
                ("total_staked", self.epoch_stakes.total_staked, i64)
            );
        }
    }

    pub fn record_vote(&mut self, slot: u64) -> Option<u64> {
        let root_slot = self.lockouts.root_slot;
        self.lockouts.process_vote(&Vote { slot });
        datapoint!(
            "locktower-vote",
            ("latest", slot, i64),
            ("root", self.lockouts.root_slot.unwrap_or(0), i64)
        );
        if root_slot != self.lockouts.root_slot {
            Some(self.lockouts.root_slot.unwrap())
        } else {
            None
        }
    }

    pub fn recent_votes(&self) -> Vec<Vote> {
        let start = self.lockouts.votes.len().saturating_sub(MAX_RECENT_VOTES);
        (start..self.lockouts.votes.len())
            .map(|i| Vote::new(self.lockouts.votes[i].slot))
            .collect()
    }

    pub fn root(&self) -> Option<u64> {
        self.lockouts.root_slot
    }

    pub fn calculate_weight(&self, stake_lockouts: &HashMap<u64, StakeLockout>) -> u128 {
        let mut sum = 0u128;
        let root_slot = self.lockouts.root_slot.unwrap_or(0);
        for (slot, stake_lockout) in stake_lockouts {
            if self.lockouts.root_slot.is_some() && *slot <= root_slot {
                continue;
            }
            sum += u128::from(stake_lockout.lockout) * u128::from(stake_lockout.stake)
        }
        sum
    }

    pub fn has_voted(&self, slot: u64) -> bool {
        for vote in &self.lockouts.votes {
            if vote.slot == slot {
                return true;
            }
        }
        false
    }

    pub fn is_locked_out(&self, slot: u64, descendants: &HashMap<u64, HashSet<u64>>) -> bool {
        let mut lockouts = self.lockouts.clone();
        lockouts.process_vote(&Vote { slot });
        for vote in &lockouts.votes {
            if vote.slot == slot {
                continue;
            }
            if !descendants[&vote.slot].contains(&slot) {
                return true;
            }
        }
        if let Some(root) = lockouts.root_slot {
            !descendants[&root].contains(&slot)
        } else {
            false
        }
    }

    pub fn check_vote_stake_threshold(
        &self,
        slot: u64,
        stake_lockouts: &HashMap<u64, StakeLockout>,
    ) -> bool {
        let mut lockouts = self.lockouts.clone();
        lockouts.process_vote(&Vote { slot });
        let vote = lockouts.nth_recent_vote(self.threshold_depth);
        if let Some(vote) = vote {
            if let Some(fork_stake) = stake_lockouts.get(&vote.slot) {
                (fork_stake.stake as f64 / self.epoch_stakes.total_staked as f64)
                    > self.threshold_size
            } else {
                false
            }
        } else {
            true
        }
    }

    /// Update lockouts for all the ancestors
    fn update_ancestor_lockouts(
        stake_lockouts: &mut HashMap<u64, StakeLockout>,
        vote: &Lockout,
        ancestors: &HashMap<u64, HashSet<u64>>,
    ) {
        let mut slot_with_ancestors = vec![vote.slot];
        slot_with_ancestors.extend(ancestors.get(&vote.slot).unwrap_or(&HashSet::new()));
        for slot in slot_with_ancestors {
            let entry = &mut stake_lockouts.entry(slot).or_default();
            entry.lockout += vote.lockout();
        }
    }

    /// Update stake for all the ancestors.
    /// Note, stake is the same for all the ancestor.
    fn update_ancestor_stakes(
        stake_lockouts: &mut HashMap<u64, StakeLockout>,
        slot: u64,
        lamports: u64,
        ancestors: &HashMap<u64, HashSet<u64>>,
    ) {
        let mut slot_with_ancestors = vec![slot];
        slot_with_ancestors.extend(ancestors.get(&slot).unwrap_or(&HashSet::new()));
        for slot in slot_with_ancestors {
            let entry = &mut stake_lockouts.entry(slot).or_default();
            entry.stake += lamports;
        }
    }

    fn bank_weight(&self, bank: &Bank, ancestors: &HashMap<u64, HashSet<u64>>) -> u128 {
        let stake_lockouts =
            self.collect_vote_lockouts(bank.slot(), bank.vote_accounts().into_iter(), ancestors);
        self.calculate_weight(&stake_lockouts)
    }

    fn find_heaviest_bank(&self, bank_forks: &BankForks) -> Option<Arc<Bank>> {
        let ancestors = bank_forks.ancestors();
        let mut bank_weights: Vec<_> = bank_forks
            .frozen_banks()
            .values()
            .map(|b| {
                (
                    self.bank_weight(b, &ancestors),
                    b.parents().len(),
                    b.clone(),
                )
            })
            .collect();
        bank_weights.sort_by_key(|b| (b.0, b.1));
        bank_weights.pop().map(|b| b.2)
    }

    fn initialize_lockouts_from_bank(bank: &Bank, current_epoch: u64) -> VoteState {
        let mut lockouts = VoteState::default();
        if let Some(iter) = bank.epoch_vote_accounts(current_epoch) {
            for (delegate_id, (_, account)) in iter {
                if *delegate_id == bank.collector_id() {
                    let state = VoteState::deserialize(&account.data).expect("votes");
                    if lockouts.votes.len() < state.votes.len() {
                        lockouts = state;
                    }
                }
            }
        };
        lockouts
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn gen_stakes(stake_votes: &[(u64, &[u64])]) -> Vec<(Pubkey, (u64, Account))> {
        let mut stakes = vec![];
        for (lamports, votes) in stake_votes {
            let mut account = Account::default();
            account.data = vec![0; 1024];
            account.lamports = *lamports;
            let mut vote_state = VoteState::default();
            for slot in *votes {
                vote_state.process_vote(&Vote { slot: *slot });
            }
            vote_state
                .serialize(&mut account.data)
                .expect("serialize state");
            stakes.push((Pubkey::new_rand(), (*lamports, account)));
        }
        stakes
    }

    #[test]
    fn test_collect_vote_lockouts_no_epoch_stakes() {
        let accounts = gen_stakes(&[(1, &[0])]);
        let epoch_stakes = EpochStakes::new_for_tests(2);
        let locktower = Locktower::new(epoch_stakes, 0, 0.67);
        let ancestors = vec![(1, vec![0].into_iter().collect()), (0, HashSet::new())]
            .into_iter()
            .collect();
        let staked_lockouts = locktower.collect_vote_lockouts(1, accounts.into_iter(), &ancestors);
        assert!(staked_lockouts.is_empty());
    }

    #[test]
    fn test_collect_vote_lockouts_sums() {
        //two accounts voting for slot 0 with 1 token staked
        let accounts = gen_stakes(&[(1, &[0]), (1, &[0])]);
        let epoch_stakes = EpochStakes::new_from_stakes(0, &accounts);
        let locktower = Locktower::new(epoch_stakes, 0, 0.67);
        let ancestors = vec![(1, vec![0].into_iter().collect()), (0, HashSet::new())]
            .into_iter()
            .collect();
        let staked_lockouts = locktower.collect_vote_lockouts(1, accounts.into_iter(), &ancestors);
        assert_eq!(staked_lockouts[&0].stake, 2);
        assert_eq!(staked_lockouts[&0].lockout, 2 + 2 + 4 + 4);
    }

    #[test]
    fn test_collect_vote_lockouts_root() {
        let votes: Vec<u64> = (0..MAX_LOCKOUT_HISTORY as u64).into_iter().collect();
        //two accounts voting for slot 0 with 1 token staked
        let accounts = gen_stakes(&[(1, &votes), (1, &votes)]);
        let epoch_stakes = EpochStakes::new_from_stakes(0, &accounts);
        let mut locktower = Locktower::new(epoch_stakes, 0, 0.67);
        let mut ancestors = HashMap::new();
        for i in 0..(MAX_LOCKOUT_HISTORY + 1) {
            locktower.record_vote(i as u64);
            ancestors.insert(i as u64, (0..i as u64).into_iter().collect());
        }
        assert_eq!(locktower.lockouts.root_slot, Some(0));
        let staked_lockouts = locktower.collect_vote_lockouts(
            MAX_LOCKOUT_HISTORY as u64,
            accounts.into_iter(),
            &ancestors,
        );
        for i in 0..MAX_LOCKOUT_HISTORY {
            assert_eq!(staked_lockouts[&(i as u64)].stake, 2);
        }
        // should be the sum of all the weights for root
        assert!(staked_lockouts[&0].lockout > (2 * (1 << MAX_LOCKOUT_HISTORY)));
    }

    #[test]
    fn test_calculate_weight_skips_root() {
        let mut locktower = Locktower::new(EpochStakes::new_for_tests(2), 0, 0.67);
        locktower.lockouts.root_slot = Some(1);
        let stakes = vec![
            (
                0,
                StakeLockout {
                    stake: 1,
                    lockout: 8,
                },
            ),
            (
                1,
                StakeLockout {
                    stake: 1,
                    lockout: 8,
                },
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(locktower.calculate_weight(&stakes), 0u128);
    }

    #[test]
    fn test_calculate_weight() {
        let locktower = Locktower::new(EpochStakes::new_for_tests(2), 0, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 1,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        assert_eq!(locktower.calculate_weight(&stakes), 8u128);
    }

    #[test]
    fn test_check_vote_threshold_without_votes() {
        let locktower = Locktower::new(EpochStakes::new_for_tests(2), 1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 1,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        assert!(locktower.check_vote_stake_threshold(0, &stakes));
    }

    #[test]
    fn test_is_slot_confirmed_not_enough_stake_failure() {
        let locktower = Locktower::new(EpochStakes::new_for_tests(2), 1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 1,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        assert!(!locktower.is_slot_confirmed(0, &stakes));
    }

    #[test]
    fn test_is_slot_confirmed_unknown_slot() {
        let locktower = Locktower::new(EpochStakes::new_for_tests(2), 1, 0.67);
        let stakes = HashMap::new();
        assert!(!locktower.is_slot_confirmed(0, &stakes));
    }

    #[test]
    fn test_is_slot_confirmed_pass() {
        let locktower = Locktower::new(EpochStakes::new_for_tests(2), 1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 2,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        assert!(locktower.is_slot_confirmed(0, &stakes));
    }

    #[test]
    fn test_is_locked_out_empty() {
        let locktower = Locktower::new(EpochStakes::new_for_tests(2), 0, 0.67);
        let descendants = HashMap::new();
        assert!(!locktower.is_locked_out(0, &descendants));
    }

    #[test]
    fn test_is_locked_out_root_slot_child_pass() {
        let mut locktower = Locktower::new(EpochStakes::new_for_tests(2), 0, 0.67);
        let descendants = vec![(0, vec![1].into_iter().collect())]
            .into_iter()
            .collect();
        locktower.lockouts.root_slot = Some(0);
        assert!(!locktower.is_locked_out(1, &descendants));
    }

    #[test]
    fn test_is_locked_out_root_slot_sibling_fail() {
        let mut locktower = Locktower::new(EpochStakes::new_for_tests(2), 0, 0.67);
        let descendants = vec![(0, vec![1].into_iter().collect())]
            .into_iter()
            .collect();
        locktower.lockouts.root_slot = Some(0);
        assert!(locktower.is_locked_out(2, &descendants));
    }

    #[test]
    fn test_check_already_voted() {
        let mut locktower = Locktower::new(EpochStakes::new_for_tests(2), 0, 0.67);
        locktower.record_vote(0);
        assert!(locktower.has_voted(0));
        assert!(!locktower.has_voted(1));
    }

    #[test]
    fn test_is_locked_out_double_vote() {
        let mut locktower = Locktower::new(EpochStakes::new_for_tests(2), 0, 0.67);
        let descendants = vec![(0, vec![1].into_iter().collect()), (1, HashSet::new())]
            .into_iter()
            .collect();
        locktower.record_vote(0);
        locktower.record_vote(1);
        assert!(locktower.is_locked_out(0, &descendants));
    }

    #[test]
    fn test_is_locked_out_child() {
        let mut locktower = Locktower::new(EpochStakes::new_for_tests(2), 0, 0.67);
        let descendants = vec![(0, vec![1].into_iter().collect())]
            .into_iter()
            .collect();
        locktower.record_vote(0);
        assert!(!locktower.is_locked_out(1, &descendants));
    }

    #[test]
    fn test_is_locked_out_sibling() {
        let mut locktower = Locktower::new(EpochStakes::new_for_tests(2), 0, 0.67);
        let descendants = vec![
            (0, vec![1, 2].into_iter().collect()),
            (1, HashSet::new()),
            (2, HashSet::new()),
        ]
        .into_iter()
        .collect();
        locktower.record_vote(0);
        locktower.record_vote(1);
        assert!(locktower.is_locked_out(2, &descendants));
    }

    #[test]
    fn test_is_locked_out_last_vote_expired() {
        let mut locktower = Locktower::new(EpochStakes::new_for_tests(2), 0, 0.67);
        let descendants = vec![(0, vec![1, 4].into_iter().collect()), (1, HashSet::new())]
            .into_iter()
            .collect();
        locktower.record_vote(0);
        locktower.record_vote(1);
        assert!(!locktower.is_locked_out(4, &descendants));
        locktower.record_vote(4);
        assert_eq!(locktower.lockouts.votes[0].slot, 0);
        assert_eq!(locktower.lockouts.votes[0].confirmation_count, 2);
        assert_eq!(locktower.lockouts.votes[1].slot, 4);
        assert_eq!(locktower.lockouts.votes[1].confirmation_count, 1);
    }

    #[test]
    fn test_check_vote_threshold_below_threshold() {
        let mut locktower = Locktower::new(EpochStakes::new_for_tests(2), 1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 1,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        locktower.record_vote(0);
        assert!(!locktower.check_vote_stake_threshold(1, &stakes));
    }
    #[test]
    fn test_check_vote_threshold_above_threshold() {
        let mut locktower = Locktower::new(EpochStakes::new_for_tests(2), 1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 2,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        locktower.record_vote(0);
        assert!(locktower.check_vote_stake_threshold(1, &stakes));
    }

    #[test]
    fn test_check_vote_threshold_above_threshold_after_pop() {
        let mut locktower = Locktower::new(EpochStakes::new_for_tests(2), 1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 2,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        locktower.record_vote(0);
        locktower.record_vote(1);
        locktower.record_vote(2);
        assert!(locktower.check_vote_stake_threshold(6, &stakes));
    }

    #[test]
    fn test_check_vote_threshold_above_threshold_no_stake() {
        let mut locktower = Locktower::new(EpochStakes::new_for_tests(2), 1, 0.67);
        let stakes = HashMap::new();
        locktower.record_vote(0);
        assert!(!locktower.check_vote_stake_threshold(1, &stakes));
    }

    #[test]
    fn test_lockout_is_updated_for_entire_branch() {
        let mut stake_lockouts = HashMap::new();
        let vote = Lockout {
            slot: 2,
            confirmation_count: 1,
        };
        let set: HashSet<u64> = vec![0u64, 1u64].into_iter().collect();
        let mut ancestors = HashMap::new();
        ancestors.insert(2, set);
        let set: HashSet<u64> = vec![0u64].into_iter().collect();
        ancestors.insert(1, set);
        Locktower::update_ancestor_lockouts(&mut stake_lockouts, &vote, &ancestors);
        assert_eq!(stake_lockouts[&0].lockout, 2);
        assert_eq!(stake_lockouts[&1].lockout, 2);
        assert_eq!(stake_lockouts[&2].lockout, 2);
    }

    #[test]
    fn test_lockout_is_updated_for_slot_or_lower() {
        let mut stake_lockouts = HashMap::new();
        let set: HashSet<u64> = vec![0u64, 1u64].into_iter().collect();
        let mut ancestors = HashMap::new();
        ancestors.insert(2, set);
        let set: HashSet<u64> = vec![0u64].into_iter().collect();
        ancestors.insert(1, set);
        let vote = Lockout {
            slot: 2,
            confirmation_count: 1,
        };
        Locktower::update_ancestor_lockouts(&mut stake_lockouts, &vote, &ancestors);
        let vote = Lockout {
            slot: 1,
            confirmation_count: 2,
        };
        Locktower::update_ancestor_lockouts(&mut stake_lockouts, &vote, &ancestors);
        assert_eq!(stake_lockouts[&0].lockout, 2 + 4);
        assert_eq!(stake_lockouts[&1].lockout, 2 + 4);
        assert_eq!(stake_lockouts[&2].lockout, 2);
    }

    #[test]
    fn test_stake_is_updated_for_entire_branch() {
        let mut stake_lockouts = HashMap::new();
        let mut account = Account::default();
        account.lamports = 1;
        let set: HashSet<u64> = vec![0u64, 1u64].into_iter().collect();
        let ancestors: HashMap<u64, HashSet<u64>> = [(2u64, set)].into_iter().cloned().collect();
        Locktower::update_ancestor_stakes(&mut stake_lockouts, 2, account.lamports, &ancestors);
        assert_eq!(stake_lockouts[&0].stake, 1);
        assert_eq!(stake_lockouts[&1].stake, 1);
        assert_eq!(stake_lockouts[&2].stake, 1);
    }

    #[test]
    fn test_check_vote_threshold_forks() {
        // Create the ancestor relationships
        let ancestors = (0..=(VOTE_THRESHOLD_DEPTH + 1) as u64)
            .map(|slot| {
                let slot_parents: HashSet<_> = (0..slot).collect();
                (slot, slot_parents)
            })
            .collect();

        // Create votes such that
        // 1) 3/4 of the stake has voted on slot: VOTE_THRESHOLD_DEPTH - 2, lockout: 2
        // 2) 1/4 of the stake has voted on slot: VOTE_THRESHOLD_DEPTH, lockout: 2^9
        let total_stake = 4;
        let threshold_size = 0.67;
        let threshold_stake = (f64::ceil(total_stake as f64 * threshold_size)) as u64;
        let locktower_votes: Vec<u64> = (0..VOTE_THRESHOLD_DEPTH as u64).collect();
        let accounts = gen_stakes(&[
            (threshold_stake, &[(VOTE_THRESHOLD_DEPTH - 2) as u64]),
            (total_stake - threshold_stake, &locktower_votes[..]),
        ]);

        // Initialize locktower
        let stakes: HashMap<_, _> = accounts.iter().map(|(pk, (s, _))| (*pk, *s)).collect();
        let epoch_stakes = EpochStakes::new(0, stakes, &Pubkey::default());
        let mut locktower = Locktower::new(epoch_stakes, VOTE_THRESHOLD_DEPTH, threshold_size);

        // CASE 1: Record the first VOTE_THRESHOLD locktower votes for fork 2. We want to
        // evaluate a vote on slot VOTE_THRESHOLD_DEPTH. The nth most recent vote should be
        // for slot 0, which is common to all account vote states, so we should pass the
        // threshold check
        let vote_to_evaluate = VOTE_THRESHOLD_DEPTH as u64;
        for vote in &locktower_votes {
            locktower.record_vote(*vote);
        }
        let stakes_lockouts = locktower.collect_vote_lockouts(
            vote_to_evaluate,
            accounts.clone().into_iter(),
            &ancestors,
        );
        assert!(locktower.check_vote_stake_threshold(vote_to_evaluate, &stakes_lockouts));

        // CASE 2: Now we want to evaluate a vote for slot VOTE_THRESHOLD_DEPTH + 1. This slot
        // will expire the vote in one of the vote accounts, so we should have insufficient
        // stake to pass the threshold
        let vote_to_evaluate = VOTE_THRESHOLD_DEPTH as u64 + 1;
        let stakes_lockouts =
            locktower.collect_vote_lockouts(vote_to_evaluate, accounts.into_iter(), &ancestors);
        assert!(!locktower.check_vote_stake_threshold(vote_to_evaluate, &stakes_lockouts));
    }

    fn vote_and_check_recent(num_votes: usize) {
        let mut locktower = Locktower::new(EpochStakes::new_for_tests(2), 1, 0.67);
        let start = num_votes.saturating_sub(MAX_RECENT_VOTES);
        let expected: Vec<_> = (start..num_votes).map(|i| Vote::new(i as u64)).collect();
        for i in 0..num_votes {
            locktower.record_vote(i as u64);
        }
        assert_eq!(expected, locktower.recent_votes())
    }

    #[test]
    fn test_recent_votes_full() {
        vote_and_check_recent(MAX_LOCKOUT_HISTORY)
    }

    #[test]
    fn test_recent_votes_empty() {
        vote_and_check_recent(0)
    }

    #[test]
    fn test_recent_votes_exact() {
        vote_and_check_recent(MAX_RECENT_VOTES)
    }
}

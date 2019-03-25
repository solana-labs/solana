use crate::bank_forks::BankForks;
use crate::staking_utils;
use hashbrown::{HashMap, HashSet};
use solana_metrics::influxdb;
use solana_runtime::bank::Bank;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_vote_api::vote_instruction::Vote;
use solana_vote_api::vote_state::{Lockout, VoteState, MAX_LOCKOUT_HISTORY};

pub const VOTE_THRESHOLD_DEPTH: usize = 8;
pub const VOTE_THRESHOLD_SIZE: f64 = 2f64 / 3f64;

#[derive(Default)]
pub struct EpochStakes {
    slot: u64,
    stakes: HashMap<Pubkey, u64>,
    self_staked: u64,
    total_staked: u64,
}

#[derive(Default)]
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
    pub fn new(slot: u64, stakes: HashMap<Pubkey, u64>, self_id: &Pubkey) -> Self {
        let total_staked = stakes.values().sum();
        let self_staked = *stakes.get(&self_id).unwrap_or(&0);
        Self {
            slot,
            stakes,
            total_staked,
            self_staked,
        }
    }
    pub fn new_for_tests(lamports: u64) -> Self {
        Self::new(
            0,
            vec![(Pubkey::default(), lamports)].into_iter().collect(),
            &Pubkey::default(),
        )
    }
    pub fn new_from_stake_accounts(slot: u64, accounts: &[(Pubkey, Account)]) -> Self {
        let stakes = accounts.iter().map(|(k, v)| (*k, v.lamports)).collect();
        Self::new(slot, stakes, &accounts[0].0)
    }
    pub fn new_from_bank(bank: &Bank) -> Self {
        let bank_epoch = bank.get_epoch_and_slot_index(bank.slot()).0;
        let stakes = staking_utils::vote_account_balances_at_epoch(bank, bank_epoch)
            .expect("voting require a bank with stakes");
        Self::new(bank_epoch, stakes, &bank.collector_id())
    }
}

impl Locktower {
    pub fn new_from_forks(bank_forks: &BankForks) -> Self {
        //TODO: which bank to start with?
        let mut frozen_banks: Vec<_> = bank_forks.frozen_banks().values().cloned().collect();
        frozen_banks.sort_by_key(|b| (b.parents().len(), b.slot()));
        if let Some(bank) = frozen_banks.last() {
            Self::new_from_bank(bank)
        } else {
            Self::default()
        }
    }

    pub fn new_from_bank(bank: &Bank) -> Self {
        let current_epoch = bank.get_epoch_and_slot_index(bank.slot()).0;
        let mut lockouts = VoteState::default();
        if let Some(iter) = staking_utils::node_staked_accounts_at_epoch(bank, current_epoch) {
            for (delegate_id, _, account) in iter {
                if *delegate_id == bank.collector_id() {
                    let state = VoteState::deserialize(&account.data).expect("votes");
                    if lockouts.votes.len() < state.votes.len() {
                        //TODO: which state to init with?
                        lockouts = state;
                    }
                }
            }
        }
        let epoch_stakes = EpochStakes::new_from_bank(bank);
        Self {
            epoch_stakes,
            threshold_depth: VOTE_THRESHOLD_DEPTH,
            threshold_size: VOTE_THRESHOLD_SIZE,
            lockouts,
        }
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
        F: Iterator<Item = (Pubkey, Account)>,
    {
        let mut stake_lockouts = HashMap::new();
        for (key, account) in vote_accounts {
            let lamports: u64 = *self.epoch_stakes.stakes.get(&key).unwrap_or(&0);
            if lamports == 0 {
                continue;
            }
            let mut vote_state: VoteState = VoteState::deserialize(&account.data)
                .expect("bank should always have valid VoteState data");
            let start_root = vote_state.root_slot;
            vote_state.process_vote(Vote { slot: bank_slot });
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
            // each account hash a stake for all the forks in the active tree for this bank
            Self::update_ancestor_stakes(&mut stake_lockouts, bank_slot, lamports, ancestors);
        }
        stake_lockouts
    }

    pub fn is_recent_epoch(&self, bank: &Bank) -> bool {
        let bank_epoch = bank.get_epoch_and_slot_index(bank.slot()).0;
        bank_epoch >= self.epoch_stakes.slot
    }

    pub fn update_epoch(&mut self, bank: &Bank) {
        trace!(
            "updating bank epoch {} {}",
            bank.slot(),
            self.epoch_stakes.slot
        );
        let bank_epoch = bank.get_epoch_and_slot_index(bank.slot()).0;
        if bank_epoch != self.epoch_stakes.slot {
            assert!(
                self.is_recent_epoch(bank),
                "epoch_stakes cannot move backwards"
            );
            info!(
                "Locktower updated epoch bank {} {}",
                bank.slot(),
                self.epoch_stakes.slot
            );
            self.epoch_stakes = EpochStakes::new_from_bank(bank);
            solana_metrics::submit(
                influxdb::Point::new("counter-locktower-epoch")
                    .add_field(
                        "slot",
                        influxdb::Value::Integer(self.epoch_stakes.slot as i64),
                    )
                    .add_field(
                        "self_staked",
                        influxdb::Value::Integer(self.epoch_stakes.self_staked as i64),
                    )
                    .add_field(
                        "total_staked",
                        influxdb::Value::Integer(self.epoch_stakes.total_staked as i64),
                    )
                    .to_owned(),
            );
        }
    }

    pub fn record_vote(&mut self, slot: u64) -> Option<u64> {
        let root_slot = self.lockouts.root_slot;
        self.lockouts.process_vote(Vote { slot });
        solana_metrics::submit(
            influxdb::Point::new("counter-locktower-vote")
                .add_field("latest", influxdb::Value::Integer(slot as i64))
                .add_field(
                    "root",
                    influxdb::Value::Integer(self.lockouts.root_slot.unwrap_or(0) as i64),
                )
                .to_owned(),
        );
        if root_slot != self.lockouts.root_slot {
            Some(self.lockouts.root_slot.unwrap())
        } else {
            None
        }
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
        lockouts.process_vote(Vote { slot });
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
        lockouts.process_vote(Vote { slot });
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
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::signature::{Keypair, KeypairUtil};

    fn gen_accounts(stake_votes: &[(u64, &[u64])]) -> Vec<(Pubkey, Account)> {
        let mut accounts = vec![];
        for (lamports, votes) in stake_votes {
            let mut account = Account::default();
            account.data = vec![0; 1024];
            account.lamports = *lamports;
            let mut vote_state = VoteState::default();
            for slot in *votes {
                vote_state.process_vote(Vote { slot: *slot });
            }
            vote_state
                .serialize(&mut account.data)
                .expect("serialize state");
            accounts.push((Keypair::new().pubkey(), account));
        }
        accounts
    }

    #[test]
    fn test_collect_vote_lockouts_no_epoch_stakes() {
        let accounts = gen_accounts(&[(1, &[0])]);
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
        let accounts = gen_accounts(&[(1, &[0]), (1, &[0])]);
        let epoch_stakes = EpochStakes::new_from_stake_accounts(0, &accounts);
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
        let accounts = gen_accounts(&[(1, &votes), (1, &votes)]);
        let epoch_stakes = EpochStakes::new_from_stake_accounts(0, &accounts);
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
}

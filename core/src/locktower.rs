use hashbrown::{HashMap, HashSet};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_vote_api::vote_instruction::Vote;
use solana_vote_api::vote_state::{Lockout, VoteState, MAX_LOCKOUT_HISTORY};

pub struct StakeLockout {
    lockout: u64,
    stake: u64,
}

pub struct Locktower {
    pub max_stake: u64,
    threshold_depth: usize,
    threshold_size: f64,
    lockouts: VoteState,
}

impl Locktower {
    pub fn new(max_stake: u64, threshold_depth: usize, threshold_size: f64) -> Locktower {
        Locktower {
            max_stake,
            threshold_depth,
            threshold_size,
            lockouts: VoteState::default(),
        }
    }
    pub fn collect_vote_lockouts<F>(
        &self,
        bank_slot: u64,
        vote_accounts: F,
        flat_parents: &HashMap<u64, HashSet<u64>>,
    ) -> HashMap<u64, StakeLockout>
    where
        F: Iterator<Item = (Pubkey, Account)>,
    {
        let mut stake_lockouts = HashMap::new();
        for (_, account) in vote_accounts {
            let mut vote_state: VoteState = VoteState::deserialize(&account.userdata)
                .expect("bank should always have valid VoteState data");
            let start_root = vote_state.root_slot;
            vote_state.process_vote(Vote { slot: bank_slot });
            for vote in &vote_state.votes {
                Self::insert_fork_tree_lockouts(&mut stake_lockouts, &vote, flat_parents);
            }
            if start_root != vote_state.root_slot {
                if let Some(root) = start_root {
                    let vote = Lockout {
                        confirmation_count: MAX_LOCKOUT_HISTORY as u32,
                        slot: root,
                    };
                    Self::insert_fork_tree_lockouts(&mut stake_lockouts, &vote, flat_parents);
                }
            }
            if let Some(root) = vote_state.root_slot {
                let vote = Lockout {
                    confirmation_count: MAX_LOCKOUT_HISTORY as u32,
                    slot: root,
                };
                Self::insert_fork_tree_lockouts(&mut stake_lockouts, &vote, flat_parents);
            }
            // each account hash a stake for all the forks in the active tree for this bank
            Self::insert_fork_tree_stake(&mut stake_lockouts, bank_slot, &account, flat_parents);
        }
        stake_lockouts
    }

    pub fn record_vote(&mut self, slot: u64) {
        self.lockouts.process_vote(Vote { slot });
    }

    pub fn calculate_weight(&self, stake_lockouts: &HashMap<u64, StakeLockout>) -> u128 {
        let mut sum = 0u128;
        let root_slot = self.lockouts.root_slot.unwrap_or(0);
        for (slot, stake_lockout) in stake_lockouts {
            if self.lockouts.root_slot.is_some() && *slot <= root_slot {
                continue;
            }
            sum += stake_lockout.lockout as u128 * stake_lockout.stake as u128
        }
        sum
    }

    pub fn check_vote_lockout(
        &self,
        slot: u64,
        flat_children: &HashMap<u64, HashSet<u64>>,
    ) -> bool {
        for vote in &self.lockouts.votes {
            assert!(vote.slot != slot, "double vote");
            if !flat_children[&vote.slot].contains(&slot) && !vote.is_expired(slot) {
                return false;
            }
        }
        if let Some(root) = self.lockouts.root_slot {
            flat_children[&root].contains(&slot)
        } else {
            true
        }
    }

    pub fn check_vote_stake_threshold(&self, stake_lockouts: &HashMap<u64, StakeLockout>) -> bool {
        let vote = self.lockouts.nth_recent_vote(self.threshold_depth);
        if let Some(vote) = vote {
            if let Some(fork_stake) = stake_lockouts.get(&vote.slot) {
                (fork_stake.stake as f64 / self.max_stake as f64) > self.threshold_size
            } else {
                false
            }
        } else {
            true
        }
    }

    //lockout is accumulated for all the parent forks
    fn insert_fork_tree_lockouts(
        stake_lockouts: &mut HashMap<u64, StakeLockout>,
        vote: &Lockout,
        flat_parents: &HashMap<u64, HashSet<u64>>,
    ) {
        let mut fork_tree = vec![vote.slot];
        fork_tree.extend(&flat_parents[&vote.slot]);
        for slot in fork_tree {
            let entry = &mut stake_lockouts.entry(slot).or_insert(StakeLockout {
                lockout: 0,
                stake: 0,
            });
            entry.lockout += vote.lockout();
        }
    }
    //stake size is the same for all the parent forks
    fn insert_fork_tree_stake(
        stake_lockouts: &mut HashMap<u64, StakeLockout>,
        slot: u64,
        account: &Account,
        flat_parents: &HashMap<u64, HashSet<u64>>,
    ) {
        let mut fork_tree = vec![slot];
        fork_tree.extend(&flat_parents[&slot]);
        for slot in fork_tree {
            let entry = &mut stake_lockouts.entry(slot).or_insert(StakeLockout {
                lockout: 0,
                stake: 0,
            });
            entry.stake += account.lamports;
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn gen_accounts(stake_votes: &[(u64, &[u64])]) -> Vec<(Pubkey, Account)> {
        let mut accounts = vec![];
        for (lamports, votes) in stake_votes {
            let mut account = Account::default();
            account.userdata = vec![0; 1024];
            account.lamports = *lamports;
            let mut vote_state = VoteState::default();
            for slot in *votes {
                vote_state.process_vote(Vote { slot: *slot });
            }
            vote_state
                .serialize(&mut account.userdata)
                .expect("serialize state");
            accounts.push((Pubkey::default(), account));
        }
        accounts
    }

    #[test]
    fn test_collect_vote_lockouts_sums() {
        let locktower = Locktower::new(2, 0, 0.67);
        //two accounts voting for slot 0 with 1 token staked
        let accounts = gen_accounts(&[(1, &[0]), (1, &[0])]);
        let flat_parents = vec![(1, vec![0].into_iter().collect()), (0, HashSet::new())]
            .into_iter()
            .collect();
        let staked_lockouts =
            locktower.collect_vote_lockouts(1, accounts.into_iter(), &flat_parents);
        assert_eq!(staked_lockouts[&0].stake, 2);
        assert_eq!(staked_lockouts[&0].lockout, 2 + 2 + 4 + 4);
    }

    #[test]
    fn test_collect_vote_lockouts_root() {
        let mut locktower = Locktower::new(2, 0, 0.67);
        let mut flat_parents = HashMap::new();
        for i in 0..(MAX_LOCKOUT_HISTORY + 1) {
            locktower.record_vote(i as u64);
            flat_parents.insert(i as u64, (0..i as u64).into_iter().collect());
        }
        assert_eq!(locktower.lockouts.root_slot, Some(0));
        let votes: Vec<u64> = (0..MAX_LOCKOUT_HISTORY as u64).into_iter().collect();
        //two accounts voting for slot 0 with 1 token staked
        let accounts = gen_accounts(&[(1, &votes), (1, &votes)]);
        let staked_lockouts = locktower.collect_vote_lockouts(
            MAX_LOCKOUT_HISTORY as u64,
            accounts.into_iter(),
            &flat_parents,
        );
        for i in 0..MAX_LOCKOUT_HISTORY {
            assert_eq!(staked_lockouts[&(i as u64)].stake, 2);
        }
        // should be the sum of all the weights for root
        assert!(staked_lockouts[&0].lockout > (2 * (1 << MAX_LOCKOUT_HISTORY)));
    }

    #[test]
    fn test_calculate_weight_skips_root() {
        let mut locktower = Locktower::new(2, 0, 0.67);
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
        let locktower = Locktower::new(2, 0, 0.67);
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
        let locktower = Locktower::new(2, 0, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 1,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        assert!(locktower.check_vote_stake_threshold(&stakes));
    }

    #[test]
    fn test_check_vote_lockout_empty() {
        let locktower = Locktower::new(2, 0, 0.67);
        let flat_children = HashMap::new();
        assert!(locktower.check_vote_lockout(0, &flat_children));
    }

    #[test]
    fn test_check_vote_lockout_root_slot_child() {
        let mut locktower = Locktower::new(2, 0, 0.67);
        let flat_children = vec![(0, vec![1].into_iter().collect())]
            .into_iter()
            .collect();
        locktower.lockouts.root_slot = Some(0);
        assert!(locktower.check_vote_lockout(1, &flat_children));
    }

    #[test]
    fn test_check_vote_lockout_root_slot_sibling() {
        let mut locktower = Locktower::new(2, 0, 0.67);
        let flat_children = vec![(0, vec![1].into_iter().collect())]
            .into_iter()
            .collect();
        locktower.lockouts.root_slot = Some(0);
        assert!(!locktower.check_vote_lockout(2, &flat_children));
    }

    #[test]
    #[should_panic]
    fn test_check_vote_lockout_double_vote() {
        let mut locktower = Locktower::new(2, 0, 0.67);
        let flat_children = vec![(0, vec![1].into_iter().collect())]
            .into_iter()
            .collect();
        locktower.record_vote(0);
        assert!(locktower.check_vote_lockout(0, &flat_children));
    }

    #[test]
    fn test_check_vote_lockout_child() {
        let mut locktower = Locktower::new(2, 0, 0.67);
        let flat_children = vec![(0, vec![1].into_iter().collect())]
            .into_iter()
            .collect();
        locktower.record_vote(0);
        assert!(locktower.check_vote_lockout(1, &flat_children));
    }

    #[test]
    fn test_check_vote_lockout_sibling() {
        let mut locktower = Locktower::new(2, 0, 0.67);
        let flat_children = vec![(0, vec![1].into_iter().collect()), (1, HashSet::new())]
            .into_iter()
            .collect();
        locktower.record_vote(0);
        locktower.record_vote(1);
        assert!(!locktower.check_vote_lockout(2, &flat_children));
    }

    #[test]
    fn test_check_vote_lockout_last_vote_expired() {
        let mut locktower = Locktower::new(2, 0, 0.67);
        let flat_children = vec![(0, vec![1, 4].into_iter().collect()), (1, HashSet::new())]
            .into_iter()
            .collect();
        locktower.record_vote(0);
        locktower.record_vote(1);
        assert!(locktower.check_vote_lockout(4, &flat_children));
        locktower.record_vote(4);
        assert_eq!(locktower.lockouts.votes[0].slot, 0);
        assert_eq!(locktower.lockouts.votes[0].confirmation_count, 2);
        assert_eq!(locktower.lockouts.votes[1].slot, 4);
        assert_eq!(locktower.lockouts.votes[1].confirmation_count, 1);
    }

    #[test]
    fn test_check_vote_threshold_below_threshold() {
        let mut locktower = Locktower::new(2, 0, 0.67);
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
        assert!(!locktower.check_vote_stake_threshold(&stakes));
    }
    #[test]
    fn test_check_vote_threshold_above_threshold() {
        let mut locktower = Locktower::new(2, 0, 0.67);
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
        assert!(locktower.check_vote_stake_threshold(&stakes));
    }
    #[test]
    fn test_lockout_is_updated_for_entire_branch() {
        let mut stake_lockouts = HashMap::new();
        let vote = Lockout {
            slot: 2,
            confirmation_count: 1,
        };
        let set: HashSet<u64> = vec![0u64, 1u64].into_iter().collect();
        let mut flat_parents = HashMap::new();
        flat_parents.insert(2, set);
        let set: HashSet<u64> = vec![0u64].into_iter().collect();
        flat_parents.insert(1, set);
        Locktower::insert_fork_tree_lockouts(&mut stake_lockouts, &vote, &flat_parents);
        assert_eq!(stake_lockouts[&0].lockout, 2);
        assert_eq!(stake_lockouts[&1].lockout, 2);
        assert_eq!(stake_lockouts[&2].lockout, 2);
    }

    #[test]
    fn test_lockout_is_updated_for_slot_or_lower() {
        let mut stake_lockouts = HashMap::new();
        let set: HashSet<u64> = vec![0u64, 1u64].into_iter().collect();
        let mut flat_parents = HashMap::new();
        flat_parents.insert(2, set);
        let set: HashSet<u64> = vec![0u64].into_iter().collect();
        flat_parents.insert(1, set);
        let vote = Lockout {
            slot: 2,
            confirmation_count: 1,
        };
        Locktower::insert_fork_tree_lockouts(&mut stake_lockouts, &vote, &flat_parents);
        let vote = Lockout {
            slot: 1,
            confirmation_count: 2,
        };
        Locktower::insert_fork_tree_lockouts(&mut stake_lockouts, &vote, &flat_parents);
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
        let flat_parents: HashMap<u64, HashSet<u64>> = [(2u64, set)].into_iter().cloned().collect();
        Locktower::insert_fork_tree_stake(&mut stake_lockouts, 2, &account, &flat_parents);
        assert_eq!(stake_lockouts[&0].stake, 1);
        assert_eq!(stake_lockouts[&1].stake, 1);
        assert_eq!(stake_lockouts[&2].stake, 1);
    }
}

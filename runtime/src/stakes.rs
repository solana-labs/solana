//! Stakes serve as a cache of stake and vote accounts to derive
//! node stakes
use {
    crate::vote_account::{VoteAccount, VoteAccounts},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Epoch,
        pubkey::Pubkey,
        stake::{
            self,
            state::{Delegation, StakeState},
        },
        sysvar::stake_history::StakeHistory,
    },
    solana_stake_program::stake_state,
    solana_vote_program::vote_state::VoteState,
    std::{collections::HashMap, sync::Arc},
};

#[derive(Default, Clone, PartialEq, Debug, Deserialize, Serialize, AbiExample)]
pub struct Stakes {
    /// vote accounts
    vote_accounts: VoteAccounts,

    /// stake_delegations
    stake_delegations: HashMap<Pubkey, Delegation>,

    /// unused
    unused: u64,

    /// current epoch, used to calculate current stake
    epoch: Epoch,

    /// history of staking levels
    stake_history: StakeHistory,
}

impl Stakes {
    pub fn history(&self) -> &StakeHistory {
        &self.stake_history
    }
    pub fn clone_with_epoch(&self, next_epoch: Epoch) -> Self {
        let prev_epoch = self.epoch;
        if prev_epoch == next_epoch {
            self.clone()
        } else {
            // wrap up the prev epoch by adding new stake history entry for the prev epoch
            let mut stake_history_upto_prev_epoch = self.stake_history.clone();
            stake_history_upto_prev_epoch.add(
                prev_epoch,
                stake_state::new_stake_history_entry(
                    prev_epoch,
                    self.stake_delegations
                        .iter()
                        .map(|(_pubkey, stake_delegation)| stake_delegation),
                    Some(&self.stake_history),
                ),
            );

            // refresh the stake distribution of vote accounts for the next epoch, using new stake history
            let vote_accounts_for_next_epoch = self
                .vote_accounts
                .iter()
                .map(|(pubkey, (_ /*stake*/, account))| {
                    let stake = self.calculate_stake(
                        pubkey,
                        next_epoch,
                        Some(&stake_history_upto_prev_epoch),
                    );
                    (*pubkey, (stake, account.clone()))
                })
                .collect();

            Stakes {
                stake_delegations: self.stake_delegations.clone(),
                unused: self.unused,
                epoch: next_epoch,
                stake_history: stake_history_upto_prev_epoch,
                vote_accounts: vote_accounts_for_next_epoch,
            }
        }
    }

    // sum the stakes that point to the given voter_pubkey
    fn calculate_stake(
        &self,
        voter_pubkey: &Pubkey,
        epoch: Epoch,
        stake_history: Option<&StakeHistory>,
    ) -> u64 {
        self.stake_delegations
            .iter()
            .map(|(_, stake_delegation)| {
                if &stake_delegation.voter_pubkey == voter_pubkey {
                    stake_delegation.stake(epoch, stake_history)
                } else {
                    0
                }
            })
            .sum()
    }

    pub fn vote_balance_and_staked(&self) -> u64 {
        self.stake_delegations
            .iter()
            .map(|(_, stake_delegation)| stake_delegation.stake)
            .sum::<u64>()
            + self
                .vote_accounts
                .iter()
                .map(|(_pubkey, (_staked, vote_account))| vote_account.lamports())
                .sum::<u64>()
    }

    pub fn is_stake(account: &AccountSharedData) -> bool {
        solana_vote_program::check_id(account.owner())
            || stake::program::check_id(account.owner())
                && account.data().len() >= std::mem::size_of::<StakeState>()
    }

    pub fn store(
        &mut self,
        pubkey: &Pubkey,
        account: &AccountSharedData,
        check_vote_init: bool,
        remove_delegation_on_inactive: bool,
    ) -> Option<VoteAccount> {
        if solana_vote_program::check_id(account.owner()) {
            // unconditionally remove existing at first; there is no dependent calculated state for
            // votes, not like stakes (stake codepath maintains calculated stake value grouped by
            // delegated vote pubkey)
            let old = self.vote_accounts.remove(pubkey);
            // when account is removed (lamports == 0 or data uninitialized), don't read so that
            // given `pubkey` can be used for any owner in the future, while not affecting Stakes.
            if account.lamports() != 0
                && !(check_vote_init && VoteState::is_uninitialized_no_deser(account.data()))
            {
                let stake = old.as_ref().map_or_else(
                    || self.calculate_stake(pubkey, self.epoch, Some(&self.stake_history)),
                    |v| v.0,
                );

                self.vote_accounts
                    .insert(*pubkey, (stake, VoteAccount::from(account.clone())));
            }
            old.map(|(_, account)| account)
        } else if stake::program::check_id(account.owner()) {
            //  old_stake is stake lamports and voter_pubkey from the pre-store() version
            let old_stake = self.stake_delegations.get(pubkey).map(|delegation| {
                (
                    delegation.voter_pubkey,
                    delegation.stake(self.epoch, Some(&self.stake_history)),
                )
            });

            let delegation = stake_state::delegation_from(account);

            let stake = delegation.map(|delegation| {
                (
                    delegation.voter_pubkey,
                    if account.lamports() != 0 {
                        delegation.stake(self.epoch, Some(&self.stake_history))
                    } else {
                        // when account is removed (lamports == 0), this special `else` clause ensures
                        // resetting cached stake value below, even if the account happens to be
                        // still staked for some (odd) reason
                        0
                    },
                )
            });

            // if adjustments need to be made...
            if stake != old_stake {
                if let Some((voter_pubkey, stake)) = old_stake {
                    self.vote_accounts.sub_stake(&voter_pubkey, stake);
                }
                if let Some((voter_pubkey, stake)) = stake {
                    self.vote_accounts.add_stake(&voter_pubkey, stake);
                }
            }

            let remove_delegation = if remove_delegation_on_inactive {
                delegation.is_none()
            } else {
                account.lamports() == 0
            };

            if remove_delegation {
                // when account is removed (lamports == 0), remove it from Stakes as well
                // so that given `pubkey` can be used for any owner in the future, while not
                // affecting Stakes.
                self.stake_delegations.remove(pubkey);
            } else if let Some(delegation) = delegation {
                self.stake_delegations.insert(*pubkey, delegation);
            }
            None
        } else {
            // there is no need to remove possibly existing Stakes cache entries with given
            // `pubkey` because this isn't possible, first of all.
            // Runtime always enforces an intermediary write of account.lamports == 0,
            // when not-System111-owned account.owner is swapped.
            None
        }
    }

    pub fn vote_accounts(&self) -> &VoteAccounts {
        &self.vote_accounts
    }

    pub fn stake_delegations(&self) -> &HashMap<Pubkey, Delegation> {
        &self.stake_delegations
    }

    pub fn staked_nodes(&self) -> Arc<HashMap<Pubkey, u64>> {
        self.vote_accounts.staked_nodes()
    }

    pub fn highest_staked_node(&self) -> Option<Pubkey> {
        let (_pubkey, (_stake, vote_account)) = self
            .vote_accounts
            .iter()
            .max_by(|(_ak, av), (_bk, bv)| av.0.cmp(&bv.0))?;
        let node_pubkey = vote_account.vote_state().as_ref().ok()?.node_pubkey;
        Some(node_pubkey)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use solana_sdk::{account::WritableAccount, pubkey::Pubkey, rent::Rent};
    use solana_stake_program::stake_state;
    use solana_vote_program::vote_state::{self, VoteState, VoteStateVersions};

    //  set up some dummies for a staked node     ((     vote      )  (     stake     ))
    pub fn create_staked_node_accounts(
        stake: u64,
    ) -> ((Pubkey, AccountSharedData), (Pubkey, AccountSharedData)) {
        let vote_pubkey = solana_sdk::pubkey::new_rand();
        let vote_account =
            vote_state::create_account(&vote_pubkey, &solana_sdk::pubkey::new_rand(), 0, 1);
        (
            (vote_pubkey, vote_account),
            create_stake_account(stake, &vote_pubkey),
        )
    }

    //   add stake to a vote_pubkey                               (   stake    )
    pub fn create_stake_account(stake: u64, vote_pubkey: &Pubkey) -> (Pubkey, AccountSharedData) {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        (
            stake_pubkey,
            stake_state::create_account(
                &stake_pubkey,
                vote_pubkey,
                &vote_state::create_account(vote_pubkey, &solana_sdk::pubkey::new_rand(), 0, 1),
                &Rent::free(),
                stake,
            ),
        )
    }

    pub fn create_warming_staked_node_accounts(
        stake: u64,
        epoch: Epoch,
    ) -> ((Pubkey, AccountSharedData), (Pubkey, AccountSharedData)) {
        let vote_pubkey = solana_sdk::pubkey::new_rand();
        let vote_account =
            vote_state::create_account(&vote_pubkey, &solana_sdk::pubkey::new_rand(), 0, 1);
        (
            (vote_pubkey, vote_account),
            create_warming_stake_account(stake, epoch, &vote_pubkey),
        )
    }

    // add stake to a vote_pubkey                               (   stake    )
    pub fn create_warming_stake_account(
        stake: u64,
        epoch: Epoch,
        vote_pubkey: &Pubkey,
    ) -> (Pubkey, AccountSharedData) {
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        (
            stake_pubkey,
            stake_state::create_account_with_activation_epoch(
                &stake_pubkey,
                vote_pubkey,
                &vote_state::create_account(vote_pubkey, &solana_sdk::pubkey::new_rand(), 0, 1),
                &Rent::free(),
                stake,
                epoch,
            ),
        )
    }

    #[test]
    fn test_stakes_basic() {
        for i in 0..4 {
            let mut stakes = Stakes {
                epoch: i,
                ..Stakes::default()
            };

            let ((vote_pubkey, vote_account), (stake_pubkey, mut stake_account)) =
                create_staked_node_accounts(10);

            stakes.store(&vote_pubkey, &vote_account, true, true);
            stakes.store(&stake_pubkey, &stake_account, true, true);
            let stake = stake_state::stake_from(&stake_account).unwrap();
            {
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(
                    vote_accounts.get(&vote_pubkey).unwrap().0,
                    stake.stake(i, None)
                );
            }

            stake_account.set_lamports(42);
            stakes.store(&stake_pubkey, &stake_account, true, true);
            {
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(
                    vote_accounts.get(&vote_pubkey).unwrap().0,
                    stake.stake(i, None)
                ); // stays old stake, because only 10 is activated
            }

            // activate more
            let (_stake_pubkey, mut stake_account) = create_stake_account(42, &vote_pubkey);
            stakes.store(&stake_pubkey, &stake_account, true, true);
            let stake = stake_state::stake_from(&stake_account).unwrap();
            {
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(
                    vote_accounts.get(&vote_pubkey).unwrap().0,
                    stake.stake(i, None)
                ); // now stake of 42 is activated
            }

            stake_account.set_lamports(0);
            stakes.store(&stake_pubkey, &stake_account, true, true);
            {
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 0);
            }
        }
    }

    #[test]
    fn test_stakes_highest() {
        let mut stakes = Stakes::default();

        assert_eq!(stakes.highest_staked_node(), None);

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        stakes.store(&vote_pubkey, &vote_account, true, true);
        stakes.store(&stake_pubkey, &stake_account, true, true);

        let ((vote11_pubkey, vote11_account), (stake11_pubkey, stake11_account)) =
            create_staked_node_accounts(20);

        stakes.store(&vote11_pubkey, &vote11_account, true, true);
        stakes.store(&stake11_pubkey, &stake11_account, true, true);

        let vote11_node_pubkey = VoteState::from(&vote11_account).unwrap().node_pubkey;

        assert_eq!(stakes.highest_staked_node(), Some(vote11_node_pubkey))
    }

    #[test]
    fn test_stakes_vote_account_disappear_reappear() {
        let mut stakes = Stakes {
            epoch: 4,
            ..Stakes::default()
        };

        let ((vote_pubkey, mut vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        stakes.store(&vote_pubkey, &vote_account, true, true);
        stakes.store(&stake_pubkey, &stake_account, true, true);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 10);
        }

        vote_account.set_lamports(0);
        stakes.store(&vote_pubkey, &vote_account, true, true);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_none());
        }

        vote_account.set_lamports(1);
        stakes.store(&vote_pubkey, &vote_account, true, true);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 10);
        }

        // Vote account too big
        let cache_data = vote_account.data().to_vec();
        let mut pushed = vote_account.data().to_vec();
        pushed.push(0);
        vote_account.set_data(pushed);
        stakes.store(&vote_pubkey, &vote_account, true, true);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_none());
        }

        // Vote account uninitialized
        let default_vote_state = VoteState::default();
        let versioned = VoteStateVersions::new_current(default_vote_state);
        VoteState::to(&versioned, &mut vote_account).unwrap();
        stakes.store(&vote_pubkey, &vote_account, true, true);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_none());
        }

        vote_account.set_data(cache_data);
        stakes.store(&vote_pubkey, &vote_account, true, true);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 10);
        }
    }

    #[test]
    fn test_stakes_change_delegate() {
        let mut stakes = Stakes {
            epoch: 4,
            ..Stakes::default()
        };

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        let ((vote_pubkey2, vote_account2), (_stake_pubkey2, stake_account2)) =
            create_staked_node_accounts(10);

        stakes.store(&vote_pubkey, &vote_account, true, true);
        stakes.store(&vote_pubkey2, &vote_account2, true, true);

        // delegates to vote_pubkey
        stakes.store(&stake_pubkey, &stake_account, true, true);

        let stake = stake_state::stake_from(&stake_account).unwrap();

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(
                vote_accounts.get(&vote_pubkey).unwrap().0,
                stake.stake(stakes.epoch, Some(&stakes.stake_history))
            );
            assert!(vote_accounts.get(&vote_pubkey2).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey2).unwrap().0, 0);
        }

        // delegates to vote_pubkey2
        stakes.store(&stake_pubkey, &stake_account2, true, true);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 0);
            assert!(vote_accounts.get(&vote_pubkey2).is_some());
            assert_eq!(
                vote_accounts.get(&vote_pubkey2).unwrap().0,
                stake.stake(stakes.epoch, Some(&stakes.stake_history))
            );
        }
    }
    #[test]
    fn test_stakes_multiple_stakers() {
        let mut stakes = Stakes {
            epoch: 4,
            ..Stakes::default()
        };

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        let (stake_pubkey2, stake_account2) = create_stake_account(10, &vote_pubkey);

        stakes.store(&vote_pubkey, &vote_account, true, true);

        // delegates to vote_pubkey
        stakes.store(&stake_pubkey, &stake_account, true, true);
        stakes.store(&stake_pubkey2, &stake_account2, true, true);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 20);
        }
    }
    #[test]
    fn test_clone_with_epoch() {
        let mut stakes = Stakes::default();

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        stakes.store(&vote_pubkey, &vote_account, true, true);
        stakes.store(&stake_pubkey, &stake_account, true, true);
        let stake = stake_state::stake_from(&stake_account).unwrap();

        {
            let vote_accounts = stakes.vote_accounts();
            assert_eq!(
                vote_accounts.get(&vote_pubkey).unwrap().0,
                stake.stake(stakes.epoch, Some(&stakes.stake_history))
            );
        }
        let stakes = stakes.clone_with_epoch(3);
        {
            let vote_accounts = stakes.vote_accounts();
            assert_eq!(
                vote_accounts.get(&vote_pubkey).unwrap().0,
                stake.stake(stakes.epoch, Some(&stakes.stake_history))
            );
        }
    }

    #[test]
    fn test_stakes_not_delegate() {
        let mut stakes = Stakes {
            epoch: 4,
            ..Stakes::default()
        };

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        stakes.store(&vote_pubkey, &vote_account, true, true);
        stakes.store(&stake_pubkey, &stake_account, true, true);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 10);
        }

        // not a stake account, and whacks above entry
        stakes.store(
            &stake_pubkey,
            &AccountSharedData::new(1, 0, &stake::program::id()),
            true,
            true,
        );
        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 0);
        }
    }

    #[test]
    fn test_vote_balance_and_staked_empty() {
        let stakes = Stakes::default();
        assert_eq!(stakes.vote_balance_and_staked(), 0);
    }

    #[test]
    fn test_vote_balance_and_staked_normal() {
        let mut stakes = Stakes::default();
        impl Stakes {
            pub fn vote_balance_and_warmed_staked(&self) -> u64 {
                self.vote_accounts
                    .iter()
                    .map(|(_pubkey, (staked, account))| staked + account.lamports())
                    .sum()
            }
        }

        let genesis_epoch = 0;
        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_warming_staked_node_accounts(10, genesis_epoch);
        stakes.store(&vote_pubkey, &vote_account, true, true);
        stakes.store(&stake_pubkey, &stake_account, true, true);

        assert_eq!(stakes.vote_balance_and_staked(), 11);
        assert_eq!(stakes.vote_balance_and_warmed_staked(), 1);

        for (epoch, expected_warmed_stake) in ((genesis_epoch + 1)..=3).zip(&[2, 3, 4]) {
            stakes = stakes.clone_with_epoch(epoch);
            // vote_balance_and_staked() always remain to return same lamports
            // while vote_balance_and_warmed_staked() gradually increases
            assert_eq!(stakes.vote_balance_and_staked(), 11);
            assert_eq!(
                stakes.vote_balance_and_warmed_staked(),
                *expected_warmed_stake
            );
        }
    }
}

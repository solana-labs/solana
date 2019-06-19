//! Stakes serve as a cache of stake and vote accounts to derive
//! node stakes
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_stake_api::stake_state::{create_mining_pool, StakeState};
use solana_vote_api::vote_state::VoteState;
use std::collections::HashMap;

#[derive(Default, Clone, PartialEq, Debug, Deserialize, Serialize)]
pub struct Stakes {
    /// vote accounts
    vote_accounts: HashMap<Pubkey, (u64, Account)>,

    /// stake_accounts
    stake_accounts: HashMap<Pubkey, Account>,

    /// unclaimed points.
    //  a point is a credit multiplied by the stake
    points: u64,

    /// current epoch, used to calculate current stake
    epoch: u64,
}

impl Stakes {
    pub fn clone_with_epoch(&self, epoch: u64) -> Self {
        if self.epoch == epoch {
            self.clone()
        } else {
            Stakes {
                stake_accounts: self.stake_accounts.clone(),
                points: self.points,
                epoch,
                vote_accounts: self
                    .vote_accounts
                    .iter()
                    .map(|(pubkey, (_stake, account))| {
                        (
                            *pubkey,
                            (self.calculate_stake(pubkey, epoch), account.clone()),
                        )
                    })
                    .collect(),
            }
        }
    }

    // sum the stakes that point to the given voter_pubkey
    fn calculate_stake(&self, voter_pubkey: &Pubkey, epoch: u64) -> u64 {
        self.stake_accounts
            .iter()
            .map(|(_, stake_account)| {
                StakeState::stake_from(stake_account).map_or(0, |stake| {
                    if stake.voter_pubkey == *voter_pubkey {
                        stake.stake(epoch)
                    } else {
                        0
                    }
                })
            })
            .sum()
    }

    pub fn is_stake(account: &Account) -> bool {
        solana_vote_api::check_id(&account.owner) || solana_stake_api::check_id(&account.owner)
    }

    pub fn store(&mut self, pubkey: &Pubkey, account: &Account) {
        if solana_vote_api::check_id(&account.owner) {
            if account.lamports == 0 {
                self.vote_accounts.remove(pubkey);
            } else {
                let old = self.vote_accounts.get(pubkey);

                let stake = old.map_or_else(|| self.calculate_stake(pubkey, self.epoch), |v| v.0);

                // count any increase in points, can only go forward
                let old_credits = old
                    .and_then(|(_stake, old_account)| VoteState::credits_from(old_account))
                    .unwrap_or(0);

                let credits = VoteState::credits_from(account).unwrap_or(old_credits);

                self.points += credits.saturating_sub(old_credits) * stake;

                self.vote_accounts.insert(*pubkey, (stake, account.clone()));
            }
        } else if solana_stake_api::check_id(&account.owner) {
            //  old_stake is stake lamports and voter_pubkey from the pre-store() version
            let old_stake = self.stake_accounts.get(pubkey).and_then(|old_account| {
                StakeState::stake_from(old_account)
                    .map(|stake| (stake.voter_pubkey, stake.stake(self.epoch)))
            });

            let stake = StakeState::stake_from(account).map(|stake| {
                (
                    stake.voter_pubkey,
                    if account.lamports != 0 {
                        stake.stake(self.epoch)
                    } else {
                        0
                    },
                )
            });

            // if adjustments need to be made...
            if stake != old_stake {
                if let Some((voter_pubkey, stake)) = old_stake {
                    self.vote_accounts
                        .entry(voter_pubkey)
                        .and_modify(|e| e.0 -= stake);
                }
                if let Some((voter_pubkey, stake)) = stake {
                    self.vote_accounts
                        .entry(voter_pubkey)
                        .and_modify(|e| e.0 += stake);
                }
            }

            if account.lamports == 0 {
                self.stake_accounts.remove(pubkey);
            } else {
                self.stake_accounts.insert(*pubkey, account.clone());
            }
        }
    }
    pub fn vote_accounts(&self) -> &HashMap<Pubkey, (u64, Account)> {
        &self.vote_accounts
    }

    pub fn mining_pools(&self) -> impl Iterator<Item = (&Pubkey, &Account)> {
        self.stake_accounts
            .iter()
            .filter(|(_key, account)| match StakeState::from(account) {
                Some(StakeState::MiningPool { .. }) => true,
                _ => false,
            })
    }

    pub fn highest_staked_node(&self) -> Option<Pubkey> {
        self.vote_accounts
            .iter()
            .max_by(|(_ak, av), (_bk, bv)| av.0.cmp(&bv.0))
            .and_then(|(_k, (_stake, account))| VoteState::from(account))
            .map(|vote_state| vote_state.node_pubkey)
    }

    /// currently unclaimed points
    pub fn points(&mut self) -> u64 {
        self.points
    }

    /// "claims" points, resets points to 0
    pub fn claim_points(&mut self) -> u64 {
        let points = self.points;
        self.points = 0;
        points
    }

    ///  claims points
    ///  makes a pool with the lamports and points spread over those points and
    pub fn create_mining_pool(&mut self, epoch: u64, lamports: u64) -> Account {
        let points = self.claim_points();

        create_mining_pool(lamports, epoch, lamports as f64 / points as f64)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;
    use solana_stake_api::stake_state::{self, STAKE_WARMUP_EPOCHS};
    use solana_vote_api::vote_state::{self, VoteState, MAX_LOCKOUT_HISTORY};

    //  set up some dummies for a staked node     ((     vote      )  (     stake     ))
    pub fn create_staked_node_accounts(stake: u64) -> ((Pubkey, Account), (Pubkey, Account)) {
        let vote_pubkey = Pubkey::new_rand();
        let vote_account = vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 1);
        (
            (vote_pubkey, vote_account),
            create_stake_account(stake, &vote_pubkey),
        )
    }

    //   add stake to a vote_pubkey                               (   stake    )
    pub fn create_stake_account(stake: u64, vote_pubkey: &Pubkey) -> (Pubkey, Account) {
        (
            Pubkey::new_rand(),
            stake_state::create_stake_account(&vote_pubkey, &VoteState::default(), stake),
        )
    }

    #[test]
    fn test_stakes_basic() {
        for i in 0..STAKE_WARMUP_EPOCHS + 1 {
            let mut stakes = Stakes::default();
            stakes.epoch = i;

            let ((vote_pubkey, vote_account), (stake_pubkey, mut stake_account)) =
                create_staked_node_accounts(10);

            stakes.store(&vote_pubkey, &vote_account);
            stakes.store(&stake_pubkey, &stake_account);
            let stake = StakeState::stake_from(&stake_account).unwrap();
            {
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, stake.stake(i));
            }

            stake_account.lamports = 42;
            stakes.store(&stake_pubkey, &stake_account);
            {
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, stake.stake(i)); // stays old stake, because only 10 is activated
            }

            // activate more
            let (_stake_pubkey, mut stake_account) = create_stake_account(42, &vote_pubkey);
            stakes.store(&stake_pubkey, &stake_account);
            let stake = StakeState::stake_from(&stake_account).unwrap();
            {
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, stake.stake(i)); // now stake of 42 is activated
            }

            stake_account.lamports = 0;
            stakes.store(&stake_pubkey, &stake_account);
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

        stakes.store(&vote_pubkey, &vote_account);
        stakes.store(&stake_pubkey, &stake_account);

        let ((vote11_pubkey, vote11_account), (stake11_pubkey, stake11_account)) =
            create_staked_node_accounts(20);

        stakes.store(&vote11_pubkey, &vote11_account);
        stakes.store(&stake11_pubkey, &stake11_account);

        let vote11_node_pubkey = VoteState::from(&vote11_account).unwrap().node_pubkey;

        assert_eq!(stakes.highest_staked_node(), Some(vote11_node_pubkey))
    }

    #[test]
    fn test_stakes_points() {
        let mut stakes = Stakes::default();
        stakes.epoch = STAKE_WARMUP_EPOCHS + 1;

        let stake = 42;
        assert_eq!(stakes.points(), 0);
        assert_eq!(stakes.claim_points(), 0);
        assert_eq!(stakes.claim_points(), 0);

        let ((vote_pubkey, mut vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(stake);

        stakes.store(&vote_pubkey, &vote_account);
        stakes.store(&stake_pubkey, &stake_account);

        assert_eq!(stakes.points(), 0);
        assert_eq!(stakes.claim_points(), 0);

        let mut vote_state = VoteState::from(&vote_account).unwrap();
        for i in 0..MAX_LOCKOUT_HISTORY + 42 {
            vote_state.process_slot_vote_unchecked(i as u64);
            vote_state.to(&mut vote_account).unwrap();
            stakes.store(&vote_pubkey, &vote_account);
            assert_eq!(stakes.points(), vote_state.credits() * stake);
        }
        vote_account.lamports = 0;
        stakes.store(&vote_pubkey, &vote_account);
        assert_eq!(stakes.points(), vote_state.credits() * stake);

        assert_eq!(stakes.claim_points(), vote_state.credits() * stake);
        assert_eq!(stakes.claim_points(), 0);
        assert_eq!(stakes.claim_points(), 0);

        // points come out of nowhere, but don't care here ;)
        vote_account.lamports = 1;
        stakes.store(&vote_pubkey, &vote_account);
        assert_eq!(stakes.points(), vote_state.credits() * stake);

        // test going backwards, should never go backwards
        let old_vote_state = vote_state;
        let vote_account = vote_state::create_account(&vote_pubkey, &Pubkey::new_rand(), 0, 1);
        stakes.store(&vote_pubkey, &vote_account);
        assert_eq!(stakes.points(), old_vote_state.credits() * stake);
    }

    #[test]
    fn test_stakes_vote_account_disappear_reappear() {
        let mut stakes = Stakes::default();
        stakes.epoch = STAKE_WARMUP_EPOCHS + 1;

        let ((vote_pubkey, mut vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        stakes.store(&vote_pubkey, &vote_account);
        stakes.store(&stake_pubkey, &stake_account);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 10);
        }

        vote_account.lamports = 0;
        stakes.store(&vote_pubkey, &vote_account);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_none());
        }
        vote_account.lamports = 1;
        stakes.store(&vote_pubkey, &vote_account);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 10);
        }
    }

    #[test]
    fn test_stakes_change_delegate() {
        let mut stakes = Stakes::default();
        stakes.epoch = STAKE_WARMUP_EPOCHS + 1;

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        let ((vote_pubkey2, vote_account2), (_stake_pubkey2, stake_account2)) =
            create_staked_node_accounts(10);

        stakes.store(&vote_pubkey, &vote_account);
        stakes.store(&vote_pubkey2, &vote_account2);

        // delegates to vote_pubkey
        stakes.store(&stake_pubkey, &stake_account);

        let stake = StakeState::stake_from(&stake_account).unwrap();

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(
                vote_accounts.get(&vote_pubkey).unwrap().0,
                stake.stake(stakes.epoch)
            );
            assert!(vote_accounts.get(&vote_pubkey2).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey2).unwrap().0, 0);
        }

        // delegates to vote_pubkey2
        stakes.store(&stake_pubkey, &stake_account2);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 0);
            assert!(vote_accounts.get(&vote_pubkey2).is_some());
            assert_eq!(
                vote_accounts.get(&vote_pubkey2).unwrap().0,
                stake.stake(stakes.epoch)
            );
        }
    }
    #[test]
    fn test_stakes_multiple_stakers() {
        let mut stakes = Stakes::default();
        stakes.epoch = STAKE_WARMUP_EPOCHS + 1;

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        let (stake_pubkey2, stake_account2) = create_stake_account(10, &vote_pubkey);

        stakes.store(&vote_pubkey, &vote_account);

        // delegates to vote_pubkey
        stakes.store(&stake_pubkey, &stake_account);
        stakes.store(&stake_pubkey2, &stake_account2);

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

        stakes.store(&vote_pubkey, &vote_account);
        stakes.store(&stake_pubkey, &stake_account);
        let stake = StakeState::stake_from(&stake_account).unwrap();

        {
            let vote_accounts = stakes.vote_accounts();
            assert_eq!(
                vote_accounts.get(&vote_pubkey).unwrap().0,
                stake.stake(stakes.epoch)
            );
        }
        let stakes = stakes.clone_with_epoch(3);
        {
            let vote_accounts = stakes.vote_accounts();
            assert_eq!(
                vote_accounts.get(&vote_pubkey).unwrap().0,
                stake.stake(stakes.epoch)
            );
        }
    }

    #[test]
    fn test_stakes_not_delegate() {
        let mut stakes = Stakes::default();
        stakes.epoch = STAKE_WARMUP_EPOCHS + 1;

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        stakes.store(&vote_pubkey, &vote_account);
        stakes.store(&stake_pubkey, &stake_account);

        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 10);
        }

        // not a stake account, and whacks above entry
        stakes.store(&stake_pubkey, &Account::new(1, 0, &solana_stake_api::id()));
        {
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 0);
        }
    }

}

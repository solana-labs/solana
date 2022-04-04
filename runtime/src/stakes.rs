//! Stakes serve as a cache of stake and vote accounts to derive
//! node stakes
use {
    crate::{
        stake_history::StakeHistory,
        vote_account::{VoteAccount, VoteAccounts},
    },
    dashmap::DashMap,
    im::HashMap as ImHashMap,
    num_derive::ToPrimitive,
    num_traits::ToPrimitive,
    rayon::{prelude::*, ThreadPool},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::{Epoch, Slot},
        pubkey::Pubkey,
        stake::{
            self,
            state::{Delegation, StakeActivationStatus, StakeState},
        },
    },
    solana_stake_program::stake_state,
    solana_vote_program::vote_state::VoteState,
    std::{
        collections::HashMap,
        ops::Add,
        sync::{Arc, RwLock, RwLockReadGuard},
    },
};

#[derive(Debug, Clone, PartialEq, ToPrimitive)]
pub enum InvalidCacheEntryReason {
    Missing,
    BadState,
    WrongOwner,
}

#[derive(Default, Debug, Deserialize, Serialize, AbiExample)]
pub struct StakesCache(RwLock<Stakes>);

impl StakesCache {
    pub fn new(stakes: Stakes) -> Self {
        Self(RwLock::new(stakes))
    }

    pub fn stakes(&self) -> RwLockReadGuard<Stakes> {
        self.0.read().unwrap()
    }

    pub fn is_stake(account: &AccountSharedData) -> bool {
        solana_vote_program::check_id(account.owner())
            || stake::program::check_id(account.owner())
                && account.data().len() >= std::mem::size_of::<StakeState>()
    }

    pub fn check_and_store(&self, pubkey: &Pubkey, account: &AccountSharedData) {
        if solana_vote_program::check_id(account.owner()) {
            let new_vote_account = if account.lamports() != 0
                && VoteState::is_correct_size_and_initialized(account.data())
            {
                let vote_account = VoteAccount::from(account.clone());
                {
                    // Called to eagerly deserialize vote state
                    let _res = vote_account.vote_state();
                }
                Some(vote_account)
            } else {
                None
            };

            self.0
                .write()
                .unwrap()
                .update_vote_account(pubkey, new_vote_account);
        } else if solana_stake_program::check_id(account.owner()) {
            let new_delegation = stake_state::delegation_from(account).map(|delegation| {
                let stakes = self.stakes();
                let stake = if account.lamports() != 0 {
                    delegation.stake(stakes.epoch, Some(&stakes.stake_history))
                } else {
                    // when account is removed (lamports == 0), this special `else` clause ensures
                    // resetting cached stake value below, even if the account happens to be
                    // still staked for some (odd) reason
                    0
                };
                (stake, delegation)
            });

            self.0
                .write()
                .unwrap()
                .update_stake_delegation(pubkey, new_delegation);
        }
    }

    pub fn activate_epoch(&self, next_epoch: Epoch, thread_pool: &ThreadPool) {
        let mut stakes = self.0.write().unwrap();
        stakes.activate_epoch(next_epoch, thread_pool)
    }

    pub fn handle_invalid_keys(
        &self,
        invalid_stake_keys: DashMap<Pubkey, InvalidCacheEntryReason>,
        invalid_vote_keys: DashMap<Pubkey, InvalidCacheEntryReason>,
        should_evict_invalid_entries: bool,
        current_slot: Slot,
    ) {
        if invalid_stake_keys.is_empty() && invalid_vote_keys.is_empty() {
            return;
        }

        // Prune invalid stake delegations and vote accounts that were
        // not properly evicted in normal operation.
        let mut maybe_stakes = if should_evict_invalid_entries {
            Some(self.0.write().unwrap())
        } else {
            None
        };

        for (stake_pubkey, reason) in invalid_stake_keys {
            if let Some(stakes) = maybe_stakes.as_mut() {
                stakes.remove_stake_delegation(&stake_pubkey);
            }
            datapoint_warn!(
                "bank-stake_delegation_accounts-invalid-account",
                ("slot", current_slot as i64, i64),
                ("stake-address", format!("{:?}", stake_pubkey), String),
                ("reason", reason.to_i64().unwrap_or_default(), i64),
            );
        }

        for (vote_pubkey, reason) in invalid_vote_keys {
            if let Some(stakes) = maybe_stakes.as_mut() {
                stakes.remove_vote_account(&vote_pubkey);
            }
            datapoint_warn!(
                "bank-stake_delegation_accounts-invalid-account",
                ("slot", current_slot as i64, i64),
                ("vote-address", format!("{:?}", vote_pubkey), String),
                ("reason", reason.to_i64().unwrap_or_default(), i64),
            );
        }
    }
}

#[derive(Default, Clone, PartialEq, Debug, Deserialize, Serialize, AbiExample)]
pub struct Stakes {
    /// vote accounts
    vote_accounts: VoteAccounts,

    /// stake_delegations
    stake_delegations: ImHashMap<Pubkey, Delegation>,

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

    fn activate_epoch(&mut self, next_epoch: Epoch, thread_pool: &ThreadPool) {
        type StakesHashMap = HashMap</*voter:*/ Pubkey, /*stake:*/ u64>;
        fn merge(mut acc: StakesHashMap, other: StakesHashMap) -> StakesHashMap {
            if acc.len() < other.len() {
                return merge(other, acc);
            }
            for (key, stake) in other {
                *acc.entry(key).or_default() += stake;
            }
            acc
        }
        let stake_delegations: Vec<_> = self.stake_delegations.values().collect();
        // Wrap up the prev epoch by adding new stake history entry for the
        // prev epoch.
        let stake_history_entry = thread_pool.install(|| {
            stake_delegations
                .par_iter()
                .fold(StakeActivationStatus::default, |acc, delegation| {
                    acc + delegation
                        .stake_activating_and_deactivating(self.epoch, Some(&self.stake_history))
                })
                .reduce(StakeActivationStatus::default, Add::add)
        });
        self.stake_history.add(self.epoch, stake_history_entry);
        self.epoch = next_epoch;
        // Refresh the stake distribution of vote accounts for the next epoch,
        // using new stake history.
        let delegated_stakes = thread_pool.install(|| {
            stake_delegations
                .par_iter()
                .fold(HashMap::default, |mut delegated_stakes, delegation| {
                    let entry = delegated_stakes.entry(delegation.voter_pubkey).or_default();
                    *entry += delegation.stake(self.epoch, Some(&self.stake_history));
                    delegated_stakes
                })
                .reduce(HashMap::default, merge)
        });
        self.vote_accounts = self
            .vote_accounts
            .iter()
            .map(|(&vote_pubkey, (_ /*stake*/, vote_account))| {
                let delegated_stake = delegated_stakes
                    .get(&vote_pubkey)
                    .copied()
                    .unwrap_or_default();
                (vote_pubkey, (delegated_stake, vote_account.clone()))
            })
            .collect();
    }

    /// Sum the stakes that point to the given voter_pubkey
    fn calculate_stake(
        &self,
        voter_pubkey: &Pubkey,
        epoch: Epoch,
        stake_history: &StakeHistory,
    ) -> u64 {
        let matches_voter_pubkey = |(_, stake_delegation): &(&_, &Delegation)| {
            &stake_delegation.voter_pubkey == voter_pubkey
        };
        let get_stake = |(_, stake_delegation): (_, &Delegation)| {
            stake_delegation.stake(epoch, Some(stake_history))
        };

        self.stake_delegations
            .iter()
            .filter(matches_voter_pubkey)
            .map(get_stake)
            .sum()
    }

    /// Sum the lamports of the vote accounts and the delegated stake
    pub fn vote_balance_and_staked(&self) -> u64 {
        let get_stake = |(_, stake_delegation): (_, &Delegation)| stake_delegation.stake;
        let get_lamports = |(_, (_, vote_account)): (_, &(_, VoteAccount))| vote_account.lamports();

        self.stake_delegations.iter().map(get_stake).sum::<u64>()
            + self.vote_accounts.iter().map(get_lamports).sum::<u64>()
    }

    pub fn remove_vote_account(&mut self, vote_pubkey: &Pubkey) {
        self.vote_accounts.remove(vote_pubkey);
    }

    pub fn remove_stake_delegation(&mut self, stake_pubkey: &Pubkey) {
        if let Some(removed_delegation) = self.stake_delegations.remove(stake_pubkey) {
            let removed_stake = removed_delegation.stake(self.epoch, Some(&self.stake_history));
            self.vote_accounts
                .sub_stake(&removed_delegation.voter_pubkey, removed_stake);
        }
    }

    pub fn update_vote_account(
        &mut self,
        vote_pubkey: &Pubkey,
        new_vote_account: Option<VoteAccount>,
    ) {
        // unconditionally remove existing at first; there is no dependent calculated state for
        // votes, not like stakes (stake codepath maintains calculated stake value grouped by
        // delegated vote pubkey)
        let old_entry = self.vote_accounts.remove(vote_pubkey);
        if let Some(new_vote_account) = new_vote_account {
            debug_assert!(new_vote_account.is_deserialized());
            let new_stake = old_entry.as_ref().map_or_else(
                || self.calculate_stake(vote_pubkey, self.epoch, &self.stake_history),
                |(old_stake, _old_vote_account)| *old_stake,
            );

            self.vote_accounts
                .insert(*vote_pubkey, (new_stake, new_vote_account));
        }
    }

    pub fn update_stake_delegation(
        &mut self,
        stake_pubkey: &Pubkey,
        new_delegation: Option<(u64, Delegation)>,
    ) {
        //  old_stake is stake lamports and voter_pubkey from the pre-store() version
        let old_stake = self.stake_delegations.get(stake_pubkey).map(|delegation| {
            (
                delegation.voter_pubkey,
                delegation.stake(self.epoch, Some(&self.stake_history)),
            )
        });

        let new_stake = new_delegation.map(|(stake, delegation)| (delegation.voter_pubkey, stake));

        // check if adjustments need to be made...
        if new_stake != old_stake {
            if let Some((voter_pubkey, stake)) = old_stake {
                self.vote_accounts.sub_stake(&voter_pubkey, stake);
            }
            if let Some((voter_pubkey, stake)) = new_stake {
                self.vote_accounts.add_stake(&voter_pubkey, stake);
            }
        }

        if let Some((_stake, delegation)) = new_delegation {
            self.stake_delegations.insert(*stake_pubkey, delegation);
        } else {
            // when stake is no longer delegated, remove it from Stakes so that
            // given `pubkey` can be used for any owner in the future, while not
            // affecting Stakes.
            self.stake_delegations.remove(stake_pubkey);
        }
    }

    pub fn vote_accounts(&self) -> &VoteAccounts {
        &self.vote_accounts
    }

    pub(crate) fn stake_delegations(&self) -> &ImHashMap<Pubkey, Delegation> {
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
    use {
        super::*,
        rayon::ThreadPoolBuilder,
        solana_sdk::{account::WritableAccount, pubkey::Pubkey, rent::Rent},
        solana_stake_program::stake_state,
        solana_vote_program::vote_state::{self, VoteState, VoteStateVersions},
    };

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
            let stakes_cache = StakesCache::new(Stakes {
                epoch: i,
                ..Stakes::default()
            });

            let ((vote_pubkey, vote_account), (stake_pubkey, mut stake_account)) =
                create_staked_node_accounts(10);

            stakes_cache.check_and_store(&vote_pubkey, &vote_account);
            stakes_cache.check_and_store(&stake_pubkey, &stake_account);
            let stake = stake_state::stake_from(&stake_account).unwrap();
            {
                let stakes = stakes_cache.stakes();
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(
                    vote_accounts.get(&vote_pubkey).unwrap().0,
                    stake.stake(i, None)
                );
            }

            stake_account.set_lamports(42);
            stakes_cache.check_and_store(&stake_pubkey, &stake_account);
            {
                let stakes = stakes_cache.stakes();
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(
                    vote_accounts.get(&vote_pubkey).unwrap().0,
                    stake.stake(i, None)
                ); // stays old stake, because only 10 is activated
            }

            // activate more
            let (_stake_pubkey, mut stake_account) = create_stake_account(42, &vote_pubkey);
            stakes_cache.check_and_store(&stake_pubkey, &stake_account);
            let stake = stake_state::stake_from(&stake_account).unwrap();
            {
                let stakes = stakes_cache.stakes();
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(
                    vote_accounts.get(&vote_pubkey).unwrap().0,
                    stake.stake(i, None)
                ); // now stake of 42 is activated
            }

            stake_account.set_lamports(0);
            stakes_cache.check_and_store(&stake_pubkey, &stake_account);
            {
                let stakes = stakes_cache.stakes();
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 0);
            }
        }
    }

    #[test]
    fn test_stakes_highest() {
        let stakes_cache = StakesCache::default();

        assert_eq!(stakes_cache.stakes().highest_staked_node(), None);

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        stakes_cache.check_and_store(&vote_pubkey, &vote_account);
        stakes_cache.check_and_store(&stake_pubkey, &stake_account);

        let ((vote11_pubkey, vote11_account), (stake11_pubkey, stake11_account)) =
            create_staked_node_accounts(20);

        stakes_cache.check_and_store(&vote11_pubkey, &vote11_account);
        stakes_cache.check_and_store(&stake11_pubkey, &stake11_account);

        let vote11_node_pubkey = VoteState::from(&vote11_account).unwrap().node_pubkey;

        let highest_staked_node = stakes_cache.stakes().highest_staked_node();
        assert_eq!(highest_staked_node, Some(vote11_node_pubkey));
    }

    #[test]
    fn test_stakes_vote_account_disappear_reappear() {
        let stakes_cache = StakesCache::new(Stakes {
            epoch: 4,
            ..Stakes::default()
        });

        let ((vote_pubkey, mut vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        stakes_cache.check_and_store(&vote_pubkey, &vote_account);
        stakes_cache.check_and_store(&stake_pubkey, &stake_account);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 10);
        }

        vote_account.set_lamports(0);
        stakes_cache.check_and_store(&vote_pubkey, &vote_account);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_none());
        }

        vote_account.set_lamports(1);
        stakes_cache.check_and_store(&vote_pubkey, &vote_account);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 10);
        }

        // Vote account too big
        let cache_data = vote_account.data().to_vec();
        let mut pushed = vote_account.data().to_vec();
        pushed.push(0);
        vote_account.set_data(pushed);
        stakes_cache.check_and_store(&vote_pubkey, &vote_account);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_none());
        }

        // Vote account uninitialized
        let default_vote_state = VoteState::default();
        let versioned = VoteStateVersions::new_current(default_vote_state);
        VoteState::to(&versioned, &mut vote_account).unwrap();
        stakes_cache.check_and_store(&vote_pubkey, &vote_account);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_none());
        }

        vote_account.set_data(cache_data);
        stakes_cache.check_and_store(&vote_pubkey, &vote_account);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 10);
        }
    }

    #[test]
    fn test_stakes_change_delegate() {
        let stakes_cache = StakesCache::new(Stakes {
            epoch: 4,
            ..Stakes::default()
        });

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        let ((vote_pubkey2, vote_account2), (_stake_pubkey2, stake_account2)) =
            create_staked_node_accounts(10);

        stakes_cache.check_and_store(&vote_pubkey, &vote_account);
        stakes_cache.check_and_store(&vote_pubkey2, &vote_account2);

        // delegates to vote_pubkey
        stakes_cache.check_and_store(&stake_pubkey, &stake_account);

        let stake = stake_state::stake_from(&stake_account).unwrap();

        {
            let stakes = stakes_cache.stakes();
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
        stakes_cache.check_and_store(&stake_pubkey, &stake_account2);

        {
            let stakes = stakes_cache.stakes();
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
        let stakes_cache = StakesCache::new(Stakes {
            epoch: 4,
            ..Stakes::default()
        });

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        let (stake_pubkey2, stake_account2) = create_stake_account(10, &vote_pubkey);

        stakes_cache.check_and_store(&vote_pubkey, &vote_account);

        // delegates to vote_pubkey
        stakes_cache.check_and_store(&stake_pubkey, &stake_account);
        stakes_cache.check_and_store(&stake_pubkey2, &stake_account2);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 20);
        }
    }

    #[test]
    fn test_activate_epoch() {
        let stakes_cache = StakesCache::default();

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        stakes_cache.check_and_store(&vote_pubkey, &vote_account);
        stakes_cache.check_and_store(&stake_pubkey, &stake_account);
        let stake = stake_state::stake_from(&stake_account).unwrap();

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert_eq!(
                vote_accounts.get(&vote_pubkey).unwrap().0,
                stake.stake(stakes.epoch, Some(&stakes.stake_history))
            );
        }
        let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        stakes_cache.activate_epoch(3, &thread_pool);
        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert_eq!(
                vote_accounts.get(&vote_pubkey).unwrap().0,
                stake.stake(stakes.epoch, Some(&stakes.stake_history))
            );
        }
    }

    #[test]
    fn test_stakes_not_delegate() {
        let stakes_cache = StakesCache::new(Stakes {
            epoch: 4,
            ..Stakes::default()
        });

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        stakes_cache.check_and_store(&vote_pubkey, &vote_account);
        stakes_cache.check_and_store(&stake_pubkey, &stake_account);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get(&vote_pubkey).unwrap().0, 10);
        }

        // not a stake account, and whacks above entry
        stakes_cache.check_and_store(
            &stake_pubkey,
            &AccountSharedData::new(1, 0, &stake::program::id()),
        );
        {
            let stakes = stakes_cache.stakes();
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
        let stakes_cache = StakesCache::default();
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
        stakes_cache.check_and_store(&vote_pubkey, &vote_account);
        stakes_cache.check_and_store(&stake_pubkey, &stake_account);

        {
            let stakes = stakes_cache.stakes();
            assert_eq!(stakes.vote_balance_and_staked(), 11);
            assert_eq!(stakes.vote_balance_and_warmed_staked(), 1);
        }

        let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        for (epoch, expected_warmed_stake) in ((genesis_epoch + 1)..=3).zip(&[2, 3, 4]) {
            stakes_cache.activate_epoch(epoch, &thread_pool);
            // vote_balance_and_staked() always remain to return same lamports
            // while vote_balance_and_warmed_staked() gradually increases
            let stakes = stakes_cache.stakes();
            assert_eq!(stakes.vote_balance_and_staked(), 11);
            assert_eq!(
                stakes.vote_balance_and_warmed_staked(),
                *expected_warmed_stake
            );
        }
    }
}

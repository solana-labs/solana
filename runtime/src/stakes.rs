//! Stakes serve as a cache of stake and vote accounts to derive
//! node stakes
use {
    crate::{stake_account, stake_history::StakeHistory},
    dashmap::DashMap,
    im::HashMap as ImHashMap,
    log::error,
    num_derive::ToPrimitive,
    num_traits::ToPrimitive,
    rayon::{prelude::*, ThreadPool},
    solana_accounts_db::stake_rewards::StakeReward,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::{Epoch, Slot},
        pubkey::Pubkey,
        stake::state::{Delegation, StakeActivationStatus},
        vote::state::VoteStateVersions,
    },
    solana_vote::vote_account::{VoteAccount, VoteAccounts},
    std::{
        collections::{HashMap, HashSet},
        ops::{Add, Deref},
        sync::{Arc, RwLock, RwLockReadGuard},
    },
    thiserror::Error,
};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid delegation: {0}")]
    InvalidDelegation(Pubkey),
    #[error(transparent)]
    InvalidStakeAccount(#[from] stake_account::Error),
    #[error("Stake account not found: {0}")]
    StakeAccountNotFound(Pubkey),
    #[error("Vote account mismatch: {0}")]
    VoteAccountMismatch(Pubkey),
    #[error("Vote account not cached: {0}")]
    VoteAccountNotCached(Pubkey),
    #[error("Vote account not found: {0}")]
    VoteAccountNotFound(Pubkey),
}

#[derive(Debug, Clone, PartialEq, Eq, ToPrimitive)]
pub enum InvalidCacheEntryReason {
    Missing,
    BadState,
    WrongOwner,
}

type StakeAccount = stake_account::StakeAccount<Delegation>;

#[derive(Default, Debug, AbiExample)]
pub(crate) struct StakesCache(RwLock<Stakes<StakeAccount>>);

impl StakesCache {
    pub(crate) fn new(stakes: Stakes<StakeAccount>) -> Self {
        Self(RwLock::new(stakes))
    }

    pub(crate) fn stakes(&self) -> RwLockReadGuard<Stakes<StakeAccount>> {
        self.0.read().unwrap()
    }

    pub(crate) fn check_and_store(
        &self,
        pubkey: &Pubkey,
        account: &impl ReadableAccount,
        new_rate_activation_epoch: Option<Epoch>,
    ) {
        // TODO: If the account is already cached as a vote or stake account
        // but the owner changes, then this needs to evict the account from
        // the cache. see:
        // https://github.com/solana-labs/solana/pull/24200#discussion_r849935444
        let owner = account.owner();
        // Zero lamport accounts are not stored in accounts-db
        // and so should be removed from cache as well.
        if account.lamports() == 0 {
            if solana_vote_program::check_id(owner) {
                let mut stakes = self.0.write().unwrap();
                stakes.remove_vote_account(pubkey);
            } else if solana_stake_program::check_id(owner) {
                let mut stakes = self.0.write().unwrap();
                stakes.remove_stake_delegation(pubkey, new_rate_activation_epoch);
            }
            return;
        }
        debug_assert_ne!(account.lamports(), 0u64);
        if solana_vote_program::check_id(owner) {
            if VoteStateVersions::is_correct_size_and_initialized(account.data()) {
                match VoteAccount::try_from(account.to_account_shared_data()) {
                    Ok(vote_account) => {
                        {
                            // Called to eagerly deserialize vote state
                            let _res = vote_account.vote_state();
                        }
                        let mut stakes = self.0.write().unwrap();
                        stakes.upsert_vote_account(pubkey, vote_account, new_rate_activation_epoch);
                    }
                    Err(_) => {
                        let mut stakes = self.0.write().unwrap();
                        stakes.remove_vote_account(pubkey)
                    }
                }
            } else {
                let mut stakes = self.0.write().unwrap();
                stakes.remove_vote_account(pubkey)
            };
        } else if solana_stake_program::check_id(owner) {
            match StakeAccount::try_from(account.to_account_shared_data()) {
                Ok(stake_account) => {
                    let mut stakes = self.0.write().unwrap();
                    stakes.upsert_stake_delegation(
                        *pubkey,
                        stake_account,
                        new_rate_activation_epoch,
                    );
                }
                Err(_) => {
                    let mut stakes = self.0.write().unwrap();
                    stakes.remove_stake_delegation(pubkey, new_rate_activation_epoch);
                }
            }
        }
    }

    pub(crate) fn activate_epoch(
        &self,
        next_epoch: Epoch,
        thread_pool: &ThreadPool,
        new_rate_activation_epoch: Option<Epoch>,
    ) {
        let mut stakes = self.0.write().unwrap();
        stakes.activate_epoch(next_epoch, thread_pool, new_rate_activation_epoch)
    }

    pub(crate) fn update_stake_accounts(
        &self,
        thread_pool: &ThreadPool,
        stake_rewards: &[StakeReward],
        new_rate_activation_epoch: Option<Epoch>,
    ) {
        self.0.write().unwrap().update_stake_accounts(
            thread_pool,
            stake_rewards,
            new_rate_activation_epoch,
        )
    }

    pub(crate) fn handle_invalid_keys(
        &self,
        invalid_vote_keys: DashMap<Pubkey, InvalidCacheEntryReason>,
        current_slot: Slot,
    ) {
        if invalid_vote_keys.is_empty() {
            return;
        }

        // Prune invalid stake delegations and vote accounts that were
        // not properly evicted in normal operation.
        let mut stakes = self.0.write().unwrap();

        for (vote_pubkey, reason) in invalid_vote_keys {
            stakes.remove_vote_account(&vote_pubkey);
            datapoint_warn!(
                "bank-stake_delegation_accounts-invalid-account",
                ("slot", current_slot as i64, i64),
                ("vote-address", format!("{vote_pubkey:?}"), String),
                ("reason", reason.to_i64().unwrap_or_default(), i64),
            );
        }
    }
}

/// The generic type T is either Delegation or StakeAccount.
/// [`Stakes<Delegation>`] is equivalent to the old code and is used for backward
/// compatibility in [`crate::bank::BankFieldsToDeserialize`].
/// But banks cache [`Stakes<StakeAccount>`] which includes the entire stake
/// account and StakeStateV2 deserialized from the account. Doing so, will remove
/// the need to load the stake account from accounts-db when working with
/// stake-delegations.
#[derive(Default, Clone, PartialEq, Debug, Deserialize, Serialize, AbiExample)]
pub struct Stakes<T: Clone> {
    /// vote accounts
    vote_accounts: VoteAccounts,

    /// stake_delegations
    stake_delegations: ImHashMap<Pubkey, T>,

    /// unused
    unused: u64,

    /// current epoch, used to calculate current stake
    epoch: Epoch,

    /// history of staking levels
    stake_history: StakeHistory,
}

// For backward compatibility, we can only serialize and deserialize
// Stakes<Delegation>. However Bank caches Stakes<StakeAccount>. This type
// mismatch incurs a conversion cost at epoch boundary when updating
// EpochStakes.
// Below type allows EpochStakes to include either a Stakes<StakeAccount> or
// Stakes<Delegation> and so bypass the conversion cost between the two at the
// epoch boundary.
#[derive(Debug, AbiExample)]
pub enum StakesEnum {
    Accounts(Stakes<StakeAccount>),
    Delegations(Stakes<Delegation>),
}

impl<T: Clone> Stakes<T> {
    pub fn vote_accounts(&self) -> &VoteAccounts {
        &self.vote_accounts
    }

    pub(crate) fn staked_nodes(&self) -> Arc<HashMap<Pubkey, u64>> {
        self.vote_accounts.staked_nodes()
    }
}

impl Stakes<StakeAccount> {
    /// Creates a Stake<StakeAccount> from Stake<Delegation> by loading the
    /// full account state for respective stake pubkeys. get_account function
    /// should return the account at the respective slot where stakes where
    /// cached.
    pub(crate) fn new<F>(stakes: &Stakes<Delegation>, get_account: F) -> Result<Self, Error>
    where
        F: Fn(&Pubkey) -> Option<AccountSharedData>,
    {
        let stake_delegations = stakes.stake_delegations.iter().map(|(pubkey, delegation)| {
            let Some(stake_account) = get_account(pubkey) else {
                return Err(Error::StakeAccountNotFound(*pubkey));
            };
            let stake_account = StakeAccount::try_from(stake_account)?;
            // Sanity check that the delegation is consistent with what is
            // stored in the account.
            if stake_account.delegation() == *delegation {
                Ok((*pubkey, stake_account))
            } else {
                Err(Error::InvalidDelegation(*pubkey))
            }
        });
        // Assert that cached vote accounts are consistent with accounts-db.
        for (pubkey, vote_account) in stakes.vote_accounts.iter() {
            let Some(account) = get_account(pubkey) else {
                return Err(Error::VoteAccountNotFound(*pubkey));
            };
            let vote_account = vote_account.account();
            if vote_account != &account {
                error!("vote account mismatch: {pubkey}, {vote_account:?}, {account:?}");
                return Err(Error::VoteAccountMismatch(*pubkey));
            }
        }
        // Assert that all valid vote-accounts referenced in
        // stake delegations are already cached.
        let voter_pubkeys: HashSet<Pubkey> = stakes
            .stake_delegations
            .values()
            .map(|delegation| delegation.voter_pubkey)
            .filter(|voter_pubkey| stakes.vote_accounts.get(voter_pubkey).is_none())
            .collect();
        for pubkey in voter_pubkeys {
            let Some(account) = get_account(&pubkey) else {
                continue;
            };
            if VoteStateVersions::is_correct_size_and_initialized(account.data())
                && VoteAccount::try_from(account.clone()).is_ok()
            {
                error!("vote account not cached: {pubkey}, {account:?}");
                return Err(Error::VoteAccountNotCached(pubkey));
            }
        }
        Ok(Self {
            vote_accounts: stakes.vote_accounts.clone(),
            stake_delegations: stake_delegations.collect::<Result<_, _>>()?,
            unused: stakes.unused,
            epoch: stakes.epoch,
            stake_history: stakes.stake_history.clone(),
        })
    }

    pub(crate) fn history(&self) -> &StakeHistory {
        &self.stake_history
    }

    fn activate_epoch(
        &mut self,
        next_epoch: Epoch,
        thread_pool: &ThreadPool,
        new_rate_activation_epoch: Option<Epoch>,
    ) {
        let stake_delegations: Vec<_> = self.stake_delegations.values().collect();
        // Wrap up the prev epoch by adding new stake history entry for the
        // prev epoch.
        let stake_history_entry = thread_pool.install(|| {
            stake_delegations
                .par_iter()
                .fold(StakeActivationStatus::default, |acc, stake_account| {
                    let delegation = stake_account.delegation();
                    acc + delegation.stake_activating_and_deactivating(
                        self.epoch,
                        Some(&self.stake_history),
                        new_rate_activation_epoch,
                    )
                })
                .reduce(StakeActivationStatus::default, Add::add)
        });
        self.stake_history.add(self.epoch, stake_history_entry);
        self.epoch = next_epoch;
        // Refresh the stake distribution of vote accounts for the next epoch,
        // using new stake history.
        self.vote_accounts = refresh_vote_accounts(
            thread_pool,
            self.epoch,
            &self.vote_accounts,
            &stake_delegations,
            &self.stake_history,
            new_rate_activation_epoch,
        );
    }

    /// Sum the stakes that point to the given voter_pubkey
    fn calculate_stake(
        &self,
        voter_pubkey: &Pubkey,
        epoch: Epoch,
        stake_history: &StakeHistory,
        new_rate_activation_epoch: Option<Epoch>,
    ) -> u64 {
        self.stake_delegations
            .values()
            .map(StakeAccount::delegation)
            .filter(|delegation| &delegation.voter_pubkey == voter_pubkey)
            .map(|delegation| {
                delegation.stake(epoch, Some(stake_history), new_rate_activation_epoch)
            })
            .sum()
    }

    /// Sum the lamports of the vote accounts and the delegated stake
    pub(crate) fn vote_balance_and_staked(&self) -> u64 {
        let get_stake = |stake_account: &StakeAccount| stake_account.delegation().stake;
        let get_lamports = |(_, vote_account): (_, &VoteAccount)| vote_account.lamports();

        self.stake_delegations.values().map(get_stake).sum::<u64>()
            + self.vote_accounts.iter().map(get_lamports).sum::<u64>()
    }

    fn remove_vote_account(&mut self, vote_pubkey: &Pubkey) {
        self.vote_accounts.remove(vote_pubkey);
    }

    fn remove_stake_delegation(
        &mut self,
        stake_pubkey: &Pubkey,
        new_rate_activation_epoch: Option<Epoch>,
    ) {
        if let Some(stake_account) = self.stake_delegations.remove(stake_pubkey) {
            let removed_delegation = stake_account.delegation();
            let removed_stake = removed_delegation.stake(
                self.epoch,
                Some(&self.stake_history),
                new_rate_activation_epoch,
            );
            self.vote_accounts
                .sub_stake(&removed_delegation.voter_pubkey, removed_stake);
        }
    }

    fn upsert_vote_account(
        &mut self,
        vote_pubkey: &Pubkey,
        vote_account: VoteAccount,
        new_rate_activation_epoch: Option<Epoch>,
    ) {
        debug_assert_ne!(vote_account.lamports(), 0u64);
        debug_assert!(vote_account.is_deserialized());
        // unconditionally remove existing at first; there is no dependent calculated state for
        // votes, not like stakes (stake codepath maintains calculated stake value grouped by
        // delegated vote pubkey)
        let stake = match self.vote_accounts.remove(vote_pubkey) {
            None => self.calculate_stake(
                vote_pubkey,
                self.epoch,
                &self.stake_history,
                new_rate_activation_epoch,
            ),
            Some((stake, _)) => stake,
        };
        let entry = (stake, vote_account);
        self.vote_accounts.insert(*vote_pubkey, entry);
    }

    fn upsert_stake_delegation(
        &mut self,
        stake_pubkey: Pubkey,
        stake_account: StakeAccount,
        new_rate_activation_epoch: Option<Epoch>,
    ) {
        debug_assert_ne!(stake_account.lamports(), 0u64);
        let delegation = stake_account.delegation();
        let voter_pubkey = delegation.voter_pubkey;
        let stake = delegation.stake(
            self.epoch,
            Some(&self.stake_history),
            new_rate_activation_epoch,
        );
        match self.stake_delegations.insert(stake_pubkey, stake_account) {
            None => self.vote_accounts.add_stake(&voter_pubkey, stake),
            Some(old_stake_account) => {
                let old_delegation = old_stake_account.delegation();
                let old_voter_pubkey = old_delegation.voter_pubkey;
                let old_stake = old_delegation.stake(
                    self.epoch,
                    Some(&self.stake_history),
                    new_rate_activation_epoch,
                );
                if voter_pubkey != old_voter_pubkey || stake != old_stake {
                    self.vote_accounts.sub_stake(&old_voter_pubkey, old_stake);
                    self.vote_accounts.add_stake(&voter_pubkey, stake);
                }
            }
        }
    }

    fn update_stake_accounts(
        &mut self,
        thread_pool: &ThreadPool,
        stake_rewards: &[StakeReward],
        new_rate_activation_epoch: Option<Epoch>,
    ) {
        let stake_delegations: Vec<_> = thread_pool.install(|| {
            stake_rewards
                .into_par_iter()
                .filter_map(|stake_reward| {
                    let stake_account = StakeAccount::try_from(stake_reward.stake_account.clone());
                    Some((stake_reward.stake_pubkey, stake_account.ok()?))
                })
                .collect()
        });
        self.stake_delegations = std::mem::take(&mut self.stake_delegations)
            .into_iter()
            .chain(stake_delegations)
            .collect::<HashMap<Pubkey, StakeAccount>>()
            .into_iter()
            .filter(|(_, account)| account.lamports() != 0u64)
            .collect();
        let stake_delegations: Vec<_> = self.stake_delegations.values().collect();
        self.vote_accounts = refresh_vote_accounts(
            thread_pool,
            self.epoch,
            &self.vote_accounts,
            &stake_delegations,
            &self.stake_history,
            new_rate_activation_epoch,
        );
    }

    pub(crate) fn stake_delegations(&self) -> &ImHashMap<Pubkey, StakeAccount> {
        &self.stake_delegations
    }

    pub(crate) fn highest_staked_node(&self) -> Option<Pubkey> {
        let vote_account = self.vote_accounts.find_max_by_delegated_stake()?;
        vote_account.node_pubkey()
    }
}

impl StakesEnum {
    pub fn vote_accounts(&self) -> &VoteAccounts {
        match self {
            StakesEnum::Accounts(stakes) => stakes.vote_accounts(),
            StakesEnum::Delegations(stakes) => stakes.vote_accounts(),
        }
    }

    pub(crate) fn staked_nodes(&self) -> Arc<HashMap<Pubkey, u64>> {
        match self {
            StakesEnum::Accounts(stakes) => stakes.staked_nodes(),
            StakesEnum::Delegations(stakes) => stakes.staked_nodes(),
        }
    }
}

impl From<Stakes<StakeAccount>> for Stakes<Delegation> {
    fn from(stakes: Stakes<StakeAccount>) -> Self {
        let stake_delegations = stakes
            .stake_delegations
            .into_iter()
            .map(|(pubkey, stake_account)| (pubkey, stake_account.delegation()))
            .collect();
        Self {
            vote_accounts: stakes.vote_accounts,
            stake_delegations,
            unused: stakes.unused,
            epoch: stakes.epoch,
            stake_history: stakes.stake_history,
        }
    }
}

impl From<Stakes<StakeAccount>> for StakesEnum {
    fn from(stakes: Stakes<StakeAccount>) -> Self {
        Self::Accounts(stakes)
    }
}

impl From<Stakes<Delegation>> for StakesEnum {
    fn from(stakes: Stakes<Delegation>) -> Self {
        Self::Delegations(stakes)
    }
}

// Two StakesEnums are equal as long as they represent the same delegations;
// whether these delegations are stored as StakeAccounts or Delegations.
// Therefore, if one side is Stakes<StakeAccount> and the other is a
// Stakes<Delegation> we convert the former one to Stakes<Delegation> before
// comparing for equality.
impl PartialEq<StakesEnum> for StakesEnum {
    fn eq(&self, other: &StakesEnum) -> bool {
        match (self, other) {
            (Self::Accounts(stakes), Self::Accounts(other)) => stakes == other,
            (Self::Accounts(stakes), Self::Delegations(other)) => {
                let stakes = Stakes::<Delegation>::from(stakes.clone());
                &stakes == other
            }
            (Self::Delegations(stakes), Self::Accounts(other)) => {
                let other = Stakes::<Delegation>::from(other.clone());
                stakes == &other
            }
            (Self::Delegations(stakes), Self::Delegations(other)) => stakes == other,
        }
    }
}

// In order to maintain backward compatibility, the StakesEnum in EpochStakes
// and SerializableVersionedBank should be serialized as Stakes<Delegation>.
pub(crate) mod serde_stakes_enum_compat {
    use {
        super::*,
        serde::{Deserialize, Deserializer, Serialize, Serializer},
    };

    pub(crate) fn serialize<S>(stakes: &StakesEnum, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match stakes {
            StakesEnum::Accounts(stakes) => {
                let stakes = Stakes::<Delegation>::from(stakes.clone());
                stakes.serialize(serializer)
            }
            StakesEnum::Delegations(stakes) => stakes.serialize(serializer),
        }
    }

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Arc<StakesEnum>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let stakes = Stakes::<Delegation>::deserialize(deserializer)?;
        Ok(Arc::new(StakesEnum::Delegations(stakes)))
    }
}

fn refresh_vote_accounts(
    thread_pool: &ThreadPool,
    epoch: Epoch,
    vote_accounts: &VoteAccounts,
    stake_delegations: &[&StakeAccount],
    stake_history: &StakeHistory,
    new_rate_activation_epoch: Option<Epoch>,
) -> VoteAccounts {
    type StakesHashMap = HashMap</*voter:*/ Pubkey, /*stake:*/ u64>;
    fn merge(mut stakes: StakesHashMap, other: StakesHashMap) -> StakesHashMap {
        if stakes.len() < other.len() {
            return merge(other, stakes);
        }
        for (pubkey, stake) in other {
            *stakes.entry(pubkey).or_default() += stake;
        }
        stakes
    }
    let stake_history = Some(stake_history.deref());
    let delegated_stakes = thread_pool.install(|| {
        stake_delegations
            .par_iter()
            .fold(HashMap::default, |mut delegated_stakes, stake_account| {
                let delegation = stake_account.delegation();
                let entry = delegated_stakes.entry(delegation.voter_pubkey).or_default();
                *entry += delegation.stake(epoch, stake_history, new_rate_activation_epoch);
                delegated_stakes
            })
            .reduce(HashMap::default, merge)
    });
    vote_accounts
        .iter()
        .map(|(&vote_pubkey, vote_account)| {
            let delegated_stake = delegated_stakes
                .get(&vote_pubkey)
                .copied()
                .unwrap_or_default();
            (vote_pubkey, (delegated_stake, vote_account.clone()))
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use {
        super::*,
        rand::Rng,
        rayon::ThreadPoolBuilder,
        solana_sdk::{account::WritableAccount, pubkey::Pubkey, rent::Rent, stake},
        solana_stake_program::stake_state,
        solana_vote_program::vote_state::{self, VoteState, VoteStateVersions},
    };

    //  set up some dummies for a staked node     ((     vote      )  (     stake     ))
    pub(crate) fn create_staked_node_accounts(
        stake: u64,
    ) -> ((Pubkey, AccountSharedData), (Pubkey, AccountSharedData)) {
        let vote_pubkey = solana_sdk::pubkey::new_rand();
        let vote_account =
            vote_state::create_account(&vote_pubkey, &solana_sdk::pubkey::new_rand(), 0, 1);
        let stake_pubkey = solana_sdk::pubkey::new_rand();
        (
            (vote_pubkey, vote_account),
            (
                stake_pubkey,
                create_stake_account(stake, &vote_pubkey, &stake_pubkey),
            ),
        )
    }

    //   add stake to a vote_pubkey                               (   stake    )
    pub(crate) fn create_stake_account(
        stake: u64,
        vote_pubkey: &Pubkey,
        stake_pubkey: &Pubkey,
    ) -> AccountSharedData {
        stake_state::create_account(
            stake_pubkey,
            vote_pubkey,
            &vote_state::create_account(vote_pubkey, &solana_sdk::pubkey::new_rand(), 0, 1),
            &Rent::free(),
            stake,
        )
    }

    fn create_warming_staked_node_accounts(
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
    fn create_warming_stake_account(
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

            stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);
            stakes_cache.check_and_store(&stake_pubkey, &stake_account, None);
            let stake = stake_state::stake_from(&stake_account).unwrap();
            {
                let stakes = stakes_cache.stakes();
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(
                    vote_accounts.get_delegated_stake(&vote_pubkey),
                    stake.stake(i, None, None)
                );
            }

            stake_account.set_lamports(42);
            stakes_cache.check_and_store(&stake_pubkey, &stake_account, None);
            {
                let stakes = stakes_cache.stakes();
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(
                    vote_accounts.get_delegated_stake(&vote_pubkey),
                    stake.stake(i, None, None)
                ); // stays old stake, because only 10 is activated
            }

            // activate more
            let mut stake_account =
                create_stake_account(42, &vote_pubkey, &solana_sdk::pubkey::new_rand());
            stakes_cache.check_and_store(&stake_pubkey, &stake_account, None);
            let stake = stake_state::stake_from(&stake_account).unwrap();
            {
                let stakes = stakes_cache.stakes();
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(
                    vote_accounts.get_delegated_stake(&vote_pubkey),
                    stake.stake(i, None, None)
                ); // now stake of 42 is activated
            }

            stake_account.set_lamports(0);
            stakes_cache.check_and_store(&stake_pubkey, &stake_account, None);
            {
                let stakes = stakes_cache.stakes();
                let vote_accounts = stakes.vote_accounts();
                assert!(vote_accounts.get(&vote_pubkey).is_some());
                assert_eq!(vote_accounts.get_delegated_stake(&vote_pubkey), 0);
            }
        }
    }

    #[test]
    fn test_stakes_highest() {
        let stakes_cache = StakesCache::default();

        assert_eq!(stakes_cache.stakes().highest_staked_node(), None);

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);
        stakes_cache.check_and_store(&stake_pubkey, &stake_account, None);

        let ((vote11_pubkey, vote11_account), (stake11_pubkey, stake11_account)) =
            create_staked_node_accounts(20);

        stakes_cache.check_and_store(&vote11_pubkey, &vote11_account, None);
        stakes_cache.check_and_store(&stake11_pubkey, &stake11_account, None);

        let vote11_node_pubkey = vote_state::from(&vote11_account).unwrap().node_pubkey;

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

        stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);
        stakes_cache.check_and_store(&stake_pubkey, &stake_account, None);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get_delegated_stake(&vote_pubkey), 10);
        }

        vote_account.set_lamports(0);
        stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_none());
            assert_eq!(vote_accounts.get_delegated_stake(&vote_pubkey), 0);
        }

        vote_account.set_lamports(1);
        stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get_delegated_stake(&vote_pubkey), 10);
        }

        // Vote account too big
        let cache_data = vote_account.data().to_vec();
        let mut pushed = vote_account.data().to_vec();
        pushed.push(0);
        vote_account.set_data(pushed);
        stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_none());
            assert_eq!(vote_accounts.get_delegated_stake(&vote_pubkey), 0);
        }

        // Vote account uninitialized
        let default_vote_state = VoteState::default();
        let versioned = VoteStateVersions::new_current(default_vote_state);
        vote_state::to(&versioned, &mut vote_account).unwrap();
        stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_none());
            assert_eq!(vote_accounts.get_delegated_stake(&vote_pubkey), 0);
        }

        vote_account.set_data(cache_data);
        stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get_delegated_stake(&vote_pubkey), 10);
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

        stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);
        stakes_cache.check_and_store(&vote_pubkey2, &vote_account2, None);

        // delegates to vote_pubkey
        stakes_cache.check_and_store(&stake_pubkey, &stake_account, None);

        let stake = stake_state::stake_from(&stake_account).unwrap();

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(
                vote_accounts.get_delegated_stake(&vote_pubkey),
                stake.stake(stakes.epoch, Some(&stakes.stake_history), None)
            );
            assert!(vote_accounts.get(&vote_pubkey2).is_some());
            assert_eq!(vote_accounts.get_delegated_stake(&vote_pubkey2), 0);
        }

        // delegates to vote_pubkey2
        stakes_cache.check_and_store(&stake_pubkey, &stake_account2, None);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get_delegated_stake(&vote_pubkey), 0);
            assert!(vote_accounts.get(&vote_pubkey2).is_some());
            assert_eq!(
                vote_accounts.get_delegated_stake(&vote_pubkey2),
                stake.stake(stakes.epoch, Some(&stakes.stake_history), None)
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

        let stake_pubkey2 = solana_sdk::pubkey::new_rand();
        let stake_account2 = create_stake_account(10, &vote_pubkey, &stake_pubkey2);

        stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);

        // delegates to vote_pubkey
        stakes_cache.check_and_store(&stake_pubkey, &stake_account, None);
        stakes_cache.check_and_store(&stake_pubkey2, &stake_account2, None);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get_delegated_stake(&vote_pubkey), 20);
        }
    }

    #[test]
    fn test_activate_epoch() {
        let stakes_cache = StakesCache::default();

        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_staked_node_accounts(10);

        stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);
        stakes_cache.check_and_store(&stake_pubkey, &stake_account, None);
        let stake = stake_state::stake_from(&stake_account).unwrap();

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert_eq!(
                vote_accounts.get_delegated_stake(&vote_pubkey),
                stake.stake(stakes.epoch, Some(&stakes.stake_history), None)
            );
        }
        let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        stakes_cache.activate_epoch(3, &thread_pool, None);
        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert_eq!(
                vote_accounts.get_delegated_stake(&vote_pubkey),
                stake.stake(stakes.epoch, Some(&stakes.stake_history), None)
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

        stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);
        stakes_cache.check_and_store(&stake_pubkey, &stake_account, None);

        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get_delegated_stake(&vote_pubkey), 10);
        }

        // not a stake account, and whacks above entry
        stakes_cache.check_and_store(
            &stake_pubkey,
            &AccountSharedData::new(1, 0, &stake::program::id()),
            None,
        );
        {
            let stakes = stakes_cache.stakes();
            let vote_accounts = stakes.vote_accounts();
            assert!(vote_accounts.get(&vote_pubkey).is_some());
            assert_eq!(vote_accounts.get_delegated_stake(&vote_pubkey), 0);
        }
    }

    #[test]
    fn test_vote_balance_and_staked_empty() {
        let stakes = Stakes::<StakeAccount>::default();
        assert_eq!(stakes.vote_balance_and_staked(), 0);
    }

    #[test]
    fn test_vote_balance_and_staked_normal() {
        let stakes_cache = StakesCache::default();
        impl Stakes<StakeAccount> {
            fn vote_balance_and_warmed_staked(&self) -> u64 {
                let vote_balance: u64 = self
                    .vote_accounts
                    .iter()
                    .map(|(_pubkey, account)| account.lamports())
                    .sum();
                let warmed_stake: u64 = self
                    .vote_accounts
                    .delegated_stakes()
                    .map(|(_pubkey, stake)| stake)
                    .sum();
                vote_balance + warmed_stake
            }
        }

        let genesis_epoch = 0;
        let ((vote_pubkey, vote_account), (stake_pubkey, stake_account)) =
            create_warming_staked_node_accounts(10, genesis_epoch);
        stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);
        stakes_cache.check_and_store(&stake_pubkey, &stake_account, None);

        {
            let stakes = stakes_cache.stakes();
            assert_eq!(stakes.vote_balance_and_staked(), 11);
            assert_eq!(stakes.vote_balance_and_warmed_staked(), 1);
        }

        let thread_pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
        for (epoch, expected_warmed_stake) in ((genesis_epoch + 1)..=3).zip(&[2, 3, 4]) {
            stakes_cache.activate_epoch(epoch, &thread_pool, None);
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

    #[test]
    fn test_serde_stakes_enum_compat() {
        #[derive(Debug, PartialEq, Deserialize, Serialize)]
        struct Dummy {
            head: String,
            #[serde(with = "serde_stakes_enum_compat")]
            stakes: Arc<StakesEnum>,
            tail: String,
        }
        let mut rng = rand::thread_rng();
        let stakes_cache = StakesCache::new(Stakes {
            unused: rng.gen(),
            epoch: rng.gen(),
            ..Stakes::default()
        });
        for _ in 0..rng.gen_range(5usize..10) {
            let vote_pubkey = solana_sdk::pubkey::new_rand();
            let vote_account = vote_state::create_account(
                &vote_pubkey,
                &solana_sdk::pubkey::new_rand(), // node_pubkey
                rng.gen_range(0..101),           // commission
                rng.gen_range(0..1_000_000),     // lamports
            );
            stakes_cache.check_and_store(&vote_pubkey, &vote_account, None);
            for _ in 0..rng.gen_range(10usize..20) {
                let stake_pubkey = solana_sdk::pubkey::new_rand();
                let rent = Rent::with_slots_per_epoch(rng.gen());
                let stake_account = stake_state::create_account(
                    &stake_pubkey, // authorized
                    &vote_pubkey,
                    &vote_account,
                    &rent,
                    rng.gen_range(0..1_000_000), // lamports
                );
                stakes_cache.check_and_store(&stake_pubkey, &stake_account, None);
            }
        }
        let stakes: Stakes<StakeAccount> = stakes_cache.stakes().clone();
        assert!(stakes.vote_accounts.as_ref().len() >= 5);
        assert!(stakes.stake_delegations.len() >= 50);
        let dummy = Dummy {
            head: String::from("dummy-head"),
            stakes: Arc::new(StakesEnum::from(stakes.clone())),
            tail: String::from("dummy-tail"),
        };
        assert!(dummy.stakes.vote_accounts().as_ref().len() >= 5);
        let data = bincode::serialize(&dummy).unwrap();
        let other: Dummy = bincode::deserialize(&data).unwrap();
        assert_eq!(other, dummy);
        let stakes = Stakes::<Delegation>::from(stakes);
        assert!(stakes.vote_accounts.as_ref().len() >= 5);
        assert!(stakes.stake_delegations.len() >= 50);
        let other = match &*other.stakes {
            StakesEnum::Accounts(_) => panic!("wrong type!"),
            StakesEnum::Delegations(delegations) => delegations,
        };
        assert_eq!(other, &stakes)
    }
}

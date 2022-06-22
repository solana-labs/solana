use {
    itertools::Itertools,
    serde::ser::{Serialize, Serializer},
    solana_sdk::{
        account::{accounts_equal, Account, AccountSharedData, ReadableAccount},
        instruction::InstructionError,
        pubkey::Pubkey,
    },
    solana_vote_program::vote_state::VoteState,
    std::{
        cmp::Ordering,
        collections::{hash_map::Entry, HashMap},
        iter::FromIterator,
        sync::{Arc, Once, RwLock, RwLockReadGuard},
    },
    thiserror::Error,
};

// The value here does not matter. It will be overwritten
// at the first call to VoteAccount::vote_state().
const INVALID_VOTE_STATE: Result<VoteState, Error> = Err(Error::InstructionError(
    InstructionError::InvalidAccountData,
));

#[derive(Clone, Debug, PartialEq, AbiExample, Deserialize)]
#[serde(try_from = "Account")]
pub struct VoteAccount(Arc<VoteAccountInner>);

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    InstructionError(#[from] InstructionError),
    #[error("Invalid vote account owner: {0}")]
    InvalidOwner(/*owner:*/ Pubkey),
}

#[derive(Debug, AbiExample)]
struct VoteAccountInner {
    account: Account,
    vote_state: RwLock<Result<VoteState, Error>>,
    vote_state_once: Once,
}

pub type VoteAccountsHashMap = HashMap<Pubkey, (/*stake:*/ u64, VoteAccount)>;

#[derive(Debug, AbiExample, Deserialize)]
#[serde(from = "Arc<VoteAccountsHashMap>")]
pub struct VoteAccounts {
    vote_accounts: Arc<VoteAccountsHashMap>,
    // Inner Arc is meant to implement copy-on-write semantics as opposed to
    // sharing mutations (hence RwLock<Arc<...>> instead of Arc<RwLock<...>>).
    staked_nodes: RwLock<
        Arc<
            HashMap<
                Pubkey, // VoteAccount.vote_state.node_pubkey.
                u64,    // Total stake across all vote-accounts.
            >,
        >,
    >,
    staked_nodes_once: Once,
}

impl VoteAccounts {
    pub fn num_vote_accounts(&self) -> usize {
        self.vote_accounts.len()
    }

    pub fn num_staked_nodes(&self) -> usize {
        self.staked_nodes.read().unwrap().len()
    }
}

impl VoteAccount {
    pub(crate) fn lamports(&self) -> u64 {
        self.0.account.lamports()
    }

    pub(crate) fn owner(&self) -> &Pubkey {
        self.0.account.owner()
    }

    pub fn vote_state(&self) -> RwLockReadGuard<Result<VoteState, Error>> {
        let inner = &self.0;
        inner.vote_state_once.call_once(|| {
            let vote_state = VoteState::deserialize(inner.account.data());
            *inner.vote_state.write().unwrap() = vote_state.map_err(Error::from);
        });
        inner.vote_state.read().unwrap()
    }

    pub(crate) fn is_deserialized(&self) -> bool {
        self.0.vote_state_once.is_completed()
    }

    /// VoteState.node_pubkey of this vote-account.
    fn node_pubkey(&self) -> Option<Pubkey> {
        Some(self.vote_state().as_ref().ok()?.node_pubkey)
    }
}

impl VoteAccounts {
    pub(crate) fn len(&self) -> usize {
        self.vote_accounts.len()
    }

    pub fn staked_nodes(&self) -> Arc<HashMap<Pubkey, u64>> {
        self.staked_nodes_once.call_once(|| {
            let staked_nodes = self
                .vote_accounts
                .values()
                .filter(|(stake, _)| *stake != 0)
                .filter_map(|(stake, vote_account)| {
                    let node_pubkey = vote_account.node_pubkey()?;
                    Some((node_pubkey, stake))
                })
                .into_grouping_map()
                .aggregate(|acc, _node_pubkey, stake| Some(acc.unwrap_or_default() + stake));
            *self.staked_nodes.write().unwrap() = Arc::new(staked_nodes)
        });
        self.staked_nodes.read().unwrap().clone()
    }

    pub fn get(&self, pubkey: &Pubkey) -> Option<&(/*stake:*/ u64, VoteAccount)> {
        self.vote_accounts.get(pubkey)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&Pubkey, &(u64, VoteAccount))> {
        self.vote_accounts.iter()
    }

    pub(crate) fn insert(&mut self, pubkey: Pubkey, (stake, vote_account): (u64, VoteAccount)) {
        self.add_node_stake(stake, &vote_account);
        let vote_accounts = Arc::make_mut(&mut self.vote_accounts);
        if let Some((stake, vote_account)) = vote_accounts.insert(pubkey, (stake, vote_account)) {
            self.sub_node_stake(stake, &vote_account);
        }
    }

    pub(crate) fn remove(&mut self, pubkey: &Pubkey) -> Option<(u64, VoteAccount)> {
        let vote_accounts = Arc::make_mut(&mut self.vote_accounts);
        let entry = vote_accounts.remove(pubkey);
        if let Some((stake, ref vote_account)) = entry {
            self.sub_node_stake(stake, vote_account);
        }
        entry
    }

    pub(crate) fn add_stake(&mut self, pubkey: &Pubkey, delta: u64) {
        let vote_accounts = Arc::make_mut(&mut self.vote_accounts);
        if let Some((stake, vote_account)) = vote_accounts.get_mut(pubkey) {
            *stake += delta;
            let vote_account = vote_account.clone();
            self.add_node_stake(delta, &vote_account);
        }
    }

    pub(crate) fn sub_stake(&mut self, pubkey: &Pubkey, delta: u64) {
        let vote_accounts = Arc::make_mut(&mut self.vote_accounts);
        if let Some((stake, vote_account)) = vote_accounts.get_mut(pubkey) {
            *stake = stake
                .checked_sub(delta)
                .expect("subtraction value exceeds account's stake");
            let vote_account = vote_account.clone();
            self.sub_node_stake(delta, &vote_account);
        }
    }

    fn add_node_stake(&mut self, stake: u64, vote_account: &VoteAccount) {
        if stake != 0 && self.staked_nodes_once.is_completed() {
            if let Some(node_pubkey) = vote_account.node_pubkey() {
                let mut staked_nodes = self.staked_nodes.write().unwrap();
                let staked_nodes = Arc::make_mut(&mut staked_nodes);
                staked_nodes
                    .entry(node_pubkey)
                    .and_modify(|s| *s += stake)
                    .or_insert(stake);
            }
        }
    }

    fn sub_node_stake(&mut self, stake: u64, vote_account: &VoteAccount) {
        if stake != 0 && self.staked_nodes_once.is_completed() {
            if let Some(node_pubkey) = vote_account.node_pubkey() {
                let mut staked_nodes = self.staked_nodes.write().unwrap();
                let staked_nodes = Arc::make_mut(&mut staked_nodes);
                match staked_nodes.entry(node_pubkey) {
                    Entry::Vacant(_) => panic!("this should not happen!"),
                    Entry::Occupied(mut entry) => match entry.get().cmp(&stake) {
                        Ordering::Less => panic!("subtraction value exceeds node's stake"),
                        Ordering::Equal => {
                            entry.remove_entry();
                        }
                        Ordering::Greater => *entry.get_mut() -= stake,
                    },
                }
            }
        }
    }
}

impl Serialize for VoteAccount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.account.serialize(serializer)
    }
}

impl TryFrom<AccountSharedData> for VoteAccount {
    type Error = Error;
    fn try_from(account: AccountSharedData) -> Result<Self, Self::Error> {
        Self::try_from(Account::from(account))
    }
}

impl From<VoteAccount> for AccountSharedData {
    fn from(account: VoteAccount) -> Self {
        Self::from(account.0.account.clone())
    }
}

impl TryFrom<Account> for VoteAccount {
    type Error = Error;
    fn try_from(account: Account) -> Result<Self, Self::Error> {
        let vote_account = VoteAccountInner::try_from(account)?;
        Ok(Self(Arc::new(vote_account)))
    }
}

impl TryFrom<Account> for VoteAccountInner {
    type Error = Error;
    fn try_from(account: Account) -> Result<Self, Self::Error> {
        if !solana_vote_program::check_id(account.owner()) {
            return Err(Error::InvalidOwner(*account.owner()));
        }
        Ok(Self {
            account,
            vote_state: RwLock::new(INVALID_VOTE_STATE),
            vote_state_once: Once::new(),
        })
    }
}

impl PartialEq<VoteAccountInner> for VoteAccountInner {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            account,
            vote_state: _,
            vote_state_once: _,
        } = self;
        account == &other.account
    }
}

impl PartialEq<AccountSharedData> for VoteAccount {
    fn eq(&self, other: &AccountSharedData) -> bool {
        accounts_equal(&self.0.account, other)
    }
}

impl Default for VoteAccounts {
    fn default() -> Self {
        Self {
            vote_accounts: Arc::default(),
            staked_nodes: RwLock::default(),
            staked_nodes_once: Once::new(),
        }
    }
}

impl Clone for VoteAccounts {
    fn clone(&self) -> Self {
        if self.staked_nodes_once.is_completed() {
            let staked_nodes = self.staked_nodes.read().unwrap().clone();
            let other = Self {
                vote_accounts: self.vote_accounts.clone(),
                staked_nodes: RwLock::new(staked_nodes),
                staked_nodes_once: Once::new(),
            };
            other.staked_nodes_once.call_once(|| {});
            other
        } else {
            Self {
                vote_accounts: self.vote_accounts.clone(),
                staked_nodes: RwLock::default(),
                staked_nodes_once: Once::new(),
            }
        }
    }
}

impl PartialEq<VoteAccounts> for VoteAccounts {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            vote_accounts,
            staked_nodes: _,
            staked_nodes_once: _,
        } = self;
        vote_accounts == &other.vote_accounts
    }
}

impl From<Arc<VoteAccountsHashMap>> for VoteAccounts {
    fn from(vote_accounts: Arc<VoteAccountsHashMap>) -> Self {
        Self {
            vote_accounts,
            staked_nodes: RwLock::default(),
            staked_nodes_once: Once::new(),
        }
    }
}

impl AsRef<VoteAccountsHashMap> for VoteAccounts {
    fn as_ref(&self) -> &VoteAccountsHashMap {
        &self.vote_accounts
    }
}

impl From<&VoteAccounts> for Arc<VoteAccountsHashMap> {
    fn from(vote_accounts: &VoteAccounts) -> Self {
        Arc::clone(&vote_accounts.vote_accounts)
    }
}

impl FromIterator<(Pubkey, (/*stake:*/ u64, VoteAccount))> for VoteAccounts {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = (Pubkey, (u64, VoteAccount))>,
    {
        Self::from(Arc::new(HashMap::from_iter(iter)))
    }
}

impl Serialize for VoteAccounts {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.vote_accounts.serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        bincode::Options,
        rand::Rng,
        solana_sdk::{pubkey::Pubkey, sysvar::clock::Clock},
        solana_vote_program::vote_state::{VoteInit, VoteStateVersions},
        std::iter::repeat_with,
    };

    fn new_rand_vote_account<R: Rng>(
        rng: &mut R,
        node_pubkey: Option<Pubkey>,
    ) -> (Account, VoteState) {
        let vote_init = VoteInit {
            node_pubkey: node_pubkey.unwrap_or_else(Pubkey::new_unique),
            authorized_voter: Pubkey::new_unique(),
            authorized_withdrawer: Pubkey::new_unique(),
            commission: rng.gen(),
        };
        let clock = Clock {
            slot: rng.gen(),
            epoch_start_timestamp: rng.gen(),
            epoch: rng.gen(),
            leader_schedule_epoch: rng.gen(),
            unix_timestamp: rng.gen(),
        };
        let vote_state = VoteState::new(&vote_init, &clock);
        let account = Account::new_data(
            rng.gen(), // lamports
            &VoteStateVersions::new_current(vote_state.clone()),
            &solana_vote_program::id(), // owner
        )
        .unwrap();
        (account, vote_state)
    }

    fn new_rand_vote_accounts<R: Rng>(
        rng: &mut R,
        num_nodes: usize,
    ) -> impl Iterator<Item = (Pubkey, (/*stake:*/ u64, VoteAccount))> + '_ {
        let nodes: Vec<_> = repeat_with(Pubkey::new_unique).take(num_nodes).collect();
        repeat_with(move || {
            let node = nodes[rng.gen_range(0, nodes.len())];
            let (account, _) = new_rand_vote_account(rng, Some(node));
            let stake = rng.gen_range(0, 997);
            let vote_account = VoteAccount::try_from(account).unwrap();
            (Pubkey::new_unique(), (stake, vote_account))
        })
    }

    fn staked_nodes<'a, I>(vote_accounts: I) -> HashMap<Pubkey, u64>
    where
        I: IntoIterator<Item = &'a (Pubkey, (u64, VoteAccount))>,
    {
        let mut staked_nodes = HashMap::new();
        for (_, (stake, vote_account)) in vote_accounts
            .into_iter()
            .filter(|(_, (stake, _))| *stake != 0)
        {
            if let Some(node_pubkey) = vote_account.node_pubkey() {
                staked_nodes
                    .entry(node_pubkey)
                    .and_modify(|s| *s += *stake)
                    .or_insert(*stake);
            }
        }
        staked_nodes
    }

    #[test]
    fn test_vote_account() {
        let mut rng = rand::thread_rng();
        let (account, vote_state) = new_rand_vote_account(&mut rng, None);
        let lamports = account.lamports();
        let vote_account = VoteAccount::try_from(account).unwrap();
        assert_eq!(lamports, vote_account.lamports());
        assert_eq!(vote_state, *vote_account.vote_state().as_ref().unwrap());
        // 2nd call to .vote_state() should return the cached value.
        assert_eq!(vote_state, *vote_account.vote_state().as_ref().unwrap());
    }

    #[test]
    fn test_vote_account_serialize() {
        let mut rng = rand::thread_rng();
        let (account, vote_state) = new_rand_vote_account(&mut rng, None);
        let vote_account = VoteAccount::try_from(account.clone()).unwrap();
        assert_eq!(vote_state, *vote_account.vote_state().as_ref().unwrap());
        // Assert than VoteAccount has the same wire format as Account.
        assert_eq!(
            bincode::serialize(&account).unwrap(),
            bincode::serialize(&vote_account).unwrap()
        );
    }

    #[test]
    fn test_vote_account_deserialize() {
        let mut rng = rand::thread_rng();
        let (account, vote_state) = new_rand_vote_account(&mut rng, None);
        let data = bincode::serialize(&account).unwrap();
        let vote_account = VoteAccount::try_from(account).unwrap();
        assert_eq!(vote_state, *vote_account.vote_state().as_ref().unwrap());
        let other_vote_account: VoteAccount = bincode::deserialize(&data).unwrap();
        assert_eq!(vote_account, other_vote_account);
        assert_eq!(
            vote_state,
            *other_vote_account.vote_state().as_ref().unwrap()
        );
    }

    #[test]
    fn test_vote_account_round_trip() {
        let mut rng = rand::thread_rng();
        let (account, vote_state) = new_rand_vote_account(&mut rng, None);
        let vote_account = VoteAccount::try_from(account).unwrap();
        assert_eq!(vote_state, *vote_account.vote_state().as_ref().unwrap());
        let data = bincode::serialize(&vote_account).unwrap();
        let other_vote_account: VoteAccount = bincode::deserialize(&data).unwrap();
        // Assert that serialize->deserialized returns the same VoteAccount.
        assert_eq!(vote_account, other_vote_account);
        assert_eq!(
            vote_state,
            *other_vote_account.vote_state().as_ref().unwrap()
        );
    }

    #[test]
    fn test_vote_accounts_serialize() {
        let mut rng = rand::thread_rng();
        let vote_accounts_hash_map: VoteAccountsHashMap =
            new_rand_vote_accounts(&mut rng, 64).take(1024).collect();
        let vote_accounts = VoteAccounts::from(Arc::new(vote_accounts_hash_map.clone()));
        assert!(vote_accounts.staked_nodes().len() > 32);
        assert_eq!(
            bincode::serialize(&vote_accounts).unwrap(),
            bincode::serialize(&vote_accounts_hash_map).unwrap(),
        );
        assert_eq!(
            bincode::options().serialize(&vote_accounts).unwrap(),
            bincode::options()
                .serialize(&vote_accounts_hash_map)
                .unwrap(),
        )
    }

    #[test]
    fn test_vote_accounts_deserialize() {
        let mut rng = rand::thread_rng();
        let vote_accounts_hash_map: VoteAccountsHashMap =
            new_rand_vote_accounts(&mut rng, 64).take(1024).collect();
        let data = bincode::serialize(&vote_accounts_hash_map).unwrap();
        let vote_accounts: VoteAccounts = bincode::deserialize(&data).unwrap();
        assert!(vote_accounts.staked_nodes().len() > 32);
        assert_eq!(*vote_accounts.vote_accounts, vote_accounts_hash_map);
        let data = bincode::options()
            .serialize(&vote_accounts_hash_map)
            .unwrap();
        let vote_accounts: VoteAccounts = bincode::options().deserialize(&data).unwrap();
        assert_eq!(*vote_accounts.vote_accounts, vote_accounts_hash_map);
    }

    #[test]
    fn test_staked_nodes() {
        let mut rng = rand::thread_rng();
        let mut accounts: Vec<_> = new_rand_vote_accounts(&mut rng, 64).take(1024).collect();
        let mut vote_accounts = VoteAccounts::default();
        // Add vote accounts.
        for (k, (pubkey, (stake, vote_account))) in accounts.iter().enumerate() {
            vote_accounts.insert(*pubkey, (*stake, vote_account.clone()));
            if (k + 1) % 128 == 0 {
                assert_eq!(
                    staked_nodes(&accounts[..k + 1]),
                    *vote_accounts.staked_nodes()
                );
            }
        }
        // Remove some of the vote accounts.
        for k in 0..256 {
            let index = rng.gen_range(0, accounts.len());
            let (pubkey, (_, _)) = accounts.swap_remove(index);
            vote_accounts.remove(&pubkey);
            if (k + 1) % 32 == 0 {
                assert_eq!(staked_nodes(&accounts), *vote_accounts.staked_nodes());
            }
        }
        // Modify the stakes for some of the accounts.
        for k in 0..2048 {
            let index = rng.gen_range(0, accounts.len());
            let (pubkey, (stake, _)) = &mut accounts[index];
            let new_stake = rng.gen_range(0, 997);
            if new_stake < *stake {
                vote_accounts.sub_stake(pubkey, *stake - new_stake);
            } else {
                vote_accounts.add_stake(pubkey, new_stake - *stake);
            }
            *stake = new_stake;
            if (k + 1) % 128 == 0 {
                assert_eq!(staked_nodes(&accounts), *vote_accounts.staked_nodes());
            }
        }
        // Remove everything.
        while !accounts.is_empty() {
            let index = rng.gen_range(0, accounts.len());
            let (pubkey, (_, _)) = accounts.swap_remove(index);
            vote_accounts.remove(&pubkey);
            if accounts.len() % 32 == 0 {
                assert_eq!(staked_nodes(&accounts), *vote_accounts.staked_nodes());
            }
        }
        assert!(vote_accounts.staked_nodes.read().unwrap().is_empty());
    }

    // Asserts that returned staked-nodes are copy-on-write references.
    #[test]
    fn test_staked_nodes_cow() {
        let mut rng = rand::thread_rng();
        let mut accounts = new_rand_vote_accounts(&mut rng, 64);
        // Add vote accounts.
        let mut vote_accounts = VoteAccounts::default();
        for (pubkey, (stake, vote_account)) in (&mut accounts).take(1024) {
            vote_accounts.insert(pubkey, (stake, vote_account));
        }
        let staked_nodes = vote_accounts.staked_nodes();
        let (pubkey, (more_stake, vote_account)) =
            accounts.find(|(_, (stake, _))| *stake != 0).unwrap();
        let node_pubkey = vote_account.node_pubkey().unwrap();
        vote_accounts.insert(pubkey, (more_stake, vote_account));
        assert_ne!(staked_nodes, vote_accounts.staked_nodes());
        assert_eq!(
            vote_accounts.staked_nodes()[&node_pubkey],
            more_stake + staked_nodes.get(&node_pubkey).copied().unwrap_or_default()
        );
        for (pubkey, stake) in vote_accounts.staked_nodes().iter() {
            if *pubkey != node_pubkey {
                assert_eq!(*stake, staked_nodes[pubkey]);
            } else {
                assert_eq!(
                    *stake,
                    more_stake + staked_nodes.get(pubkey).copied().unwrap_or_default()
                );
            }
        }
    }

    // Asserts that returned vote-accounts are copy-on-write references.
    #[test]
    fn test_vote_accounts_cow() {
        let mut rng = rand::thread_rng();
        let mut accounts = new_rand_vote_accounts(&mut rng, 64);
        // Add vote accounts.
        let mut vote_accounts = VoteAccounts::default();
        for (pubkey, (stake, vote_account)) in (&mut accounts).take(1024) {
            vote_accounts.insert(pubkey, (stake, vote_account));
        }
        let vote_accounts_hashmap = Arc::<VoteAccountsHashMap>::from(&vote_accounts);
        assert_eq!(vote_accounts_hashmap, vote_accounts.vote_accounts);
        assert!(Arc::ptr_eq(
            &vote_accounts_hashmap,
            &vote_accounts.vote_accounts
        ));
        let (pubkey, (more_stake, vote_account)) =
            accounts.find(|(_, (stake, _))| *stake != 0).unwrap();
        vote_accounts.insert(pubkey, (more_stake, vote_account.clone()));
        assert!(!Arc::ptr_eq(
            &vote_accounts_hashmap,
            &vote_accounts.vote_accounts
        ));
        assert_ne!(vote_accounts_hashmap, vote_accounts.vote_accounts);
        let other = (more_stake, vote_account);
        for (pk, value) in vote_accounts.iter() {
            if *pk != pubkey {
                assert_eq!(value, &vote_accounts_hashmap[pk]);
            } else {
                assert_eq!(value, &other);
            }
        }
    }
}

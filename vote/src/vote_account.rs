use {
    itertools::Itertools,
    serde::ser::{Serialize, Serializer},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        instruction::InstructionError,
        pubkey::Pubkey,
        vote::state::VoteState,
    },
    std::{
        cmp::Ordering,
        collections::{hash_map::Entry, HashMap},
        iter::FromIterator,
        mem,
        sync::{Arc, OnceLock},
    },
    thiserror::Error,
};

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "AccountSharedData")]
pub struct VoteAccount(Arc<VoteAccountInner>);

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    InstructionError(#[from] InstructionError),
    #[error("Invalid vote account owner: {0}")]
    InvalidOwner(/*owner:*/ Pubkey),
}

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Debug)]
struct VoteAccountInner {
    account: AccountSharedData,
    vote_state: OnceLock<Result<VoteState, Error>>,
}

pub type VoteAccountsHashMap = HashMap<Pubkey, (/*stake:*/ u64, VoteAccount)>;

#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[derive(Clone, Debug, Deserialize)]
#[serde(from = "Arc<VoteAccountsHashMap>")]
pub struct VoteAccounts {
    vote_accounts: Arc<VoteAccountsHashMap>,
    // Inner Arc is meant to implement copy-on-write semantics.
    staked_nodes: OnceLock<
        Arc<
            HashMap<
                Pubkey, // VoteAccount.vote_state.node_pubkey.
                u64,    // Total stake across all vote-accounts.
            >,
        >,
    >,
}

impl VoteAccount {
    pub fn account(&self) -> &AccountSharedData {
        &self.0.account
    }

    pub fn lamports(&self) -> u64 {
        self.0.account.lamports()
    }

    pub fn owner(&self) -> &Pubkey {
        self.0.account.owner()
    }

    pub fn vote_state(&self) -> Result<&VoteState, &Error> {
        // VoteState::deserialize deserializes a VoteStateVersions and then
        // calls VoteStateVersions::convert_to_current.
        self.0
            .vote_state
            .get_or_init(|| VoteState::deserialize(self.0.account.data()).map_err(Error::from))
            .as_ref()
    }

    pub fn is_deserialized(&self) -> bool {
        self.0.vote_state.get().is_some()
    }

    /// VoteState.node_pubkey of this vote-account.
    pub fn node_pubkey(&self) -> Option<&Pubkey> {
        self.vote_state().ok().map(|s| &s.node_pubkey)
    }
}

impl VoteAccounts {
    pub fn len(&self) -> usize {
        self.vote_accounts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vote_accounts.is_empty()
    }

    pub fn staked_nodes(&self) -> Arc<HashMap</*node_pubkey:*/ Pubkey, /*stake:*/ u64>> {
        self.staked_nodes
            .get_or_init(|| {
                Arc::new(
                    self.vote_accounts
                        .values()
                        .filter(|(stake, _)| *stake != 0u64)
                        .filter_map(|(stake, vote_account)| {
                            Some((*vote_account.node_pubkey()?, stake))
                        })
                        .into_grouping_map()
                        .aggregate(|acc, _node_pubkey, stake| {
                            Some(acc.unwrap_or_default() + stake)
                        }),
                )
            })
            .clone()
    }

    pub fn get(&self, pubkey: &Pubkey) -> Option<&VoteAccount> {
        let (_stake, vote_account) = self.vote_accounts.get(pubkey)?;
        Some(vote_account)
    }

    pub fn get_delegated_stake(&self, pubkey: &Pubkey) -> u64 {
        self.vote_accounts
            .get(pubkey)
            .map(|(stake, _vote_account)| *stake)
            .unwrap_or_default()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Pubkey, &VoteAccount)> {
        self.vote_accounts
            .iter()
            .map(|(vote_pubkey, (_stake, vote_account))| (vote_pubkey, vote_account))
    }

    pub fn delegated_stakes(&self) -> impl Iterator<Item = (&Pubkey, u64)> {
        self.vote_accounts
            .iter()
            .map(|(vote_pubkey, (stake, _vote_account))| (vote_pubkey, *stake))
    }

    pub fn find_max_by_delegated_stake(&self) -> Option<&VoteAccount> {
        let key = |(_pubkey, (stake, _vote_account)): &(_, &(u64, _))| *stake;
        let (_pubkey, (_stake, vote_account)) = self.vote_accounts.iter().max_by_key(key)?;
        Some(vote_account)
    }

    pub fn insert(
        &mut self,
        pubkey: Pubkey,
        new_vote_account: VoteAccount,
        calculate_stake: impl FnOnce() -> u64,
    ) -> Option<VoteAccount> {
        let vote_accounts = Arc::make_mut(&mut self.vote_accounts);
        match vote_accounts.entry(pubkey) {
            Entry::Occupied(mut entry) => {
                // This is an upsert, we need to update the vote state and move the stake if needed.
                let (stake, old_vote_account) = entry.get_mut();

                if let Some(staked_nodes) = self.staked_nodes.get_mut() {
                    let old_node_pubkey = old_vote_account.node_pubkey();
                    let new_node_pubkey = new_vote_account.node_pubkey();
                    if new_node_pubkey != old_node_pubkey {
                        // The node keys have changed, we move the stake from the old node to the
                        // new one
                        Self::do_sub_node_stake(staked_nodes, *stake, old_node_pubkey);
                        Self::do_add_node_stake(staked_nodes, *stake, new_node_pubkey.copied());
                    }
                }

                // Update the vote state
                Some(mem::replace(old_vote_account, new_vote_account))
            }
            Entry::Vacant(entry) => {
                // This is a new vote account. We don't know the stake yet, so we need to compute it.
                let (stake, vote_account) = entry.insert((calculate_stake(), new_vote_account));
                if let Some(staked_nodes) = self.staked_nodes.get_mut() {
                    Self::do_add_node_stake(
                        staked_nodes,
                        *stake,
                        vote_account.node_pubkey().copied(),
                    );
                }
                None
            }
        }
    }

    pub fn remove(&mut self, pubkey: &Pubkey) -> Option<(u64, VoteAccount)> {
        let vote_accounts = Arc::make_mut(&mut self.vote_accounts);
        let entry = vote_accounts.remove(pubkey);
        if let Some((stake, ref vote_account)) = entry {
            self.sub_node_stake(stake, vote_account);
        }
        entry
    }

    pub fn add_stake(&mut self, pubkey: &Pubkey, delta: u64) {
        let vote_accounts = Arc::make_mut(&mut self.vote_accounts);
        if let Some((stake, vote_account)) = vote_accounts.get_mut(pubkey) {
            *stake += delta;
            let vote_account = vote_account.clone();
            self.add_node_stake(delta, &vote_account);
        }
    }

    pub fn sub_stake(&mut self, pubkey: &Pubkey, delta: u64) {
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
        let Some(staked_nodes) = self.staked_nodes.get_mut() else {
            return;
        };

        VoteAccounts::do_add_node_stake(staked_nodes, stake, vote_account.node_pubkey().copied());
    }

    fn do_add_node_stake(
        staked_nodes: &mut Arc<HashMap<Pubkey, u64>>,
        stake: u64,
        node_pubkey: Option<Pubkey>,
    ) {
        if stake == 0u64 {
            return;
        }

        node_pubkey.map(|node_pubkey| {
            Arc::make_mut(staked_nodes)
                .entry(node_pubkey)
                .and_modify(|s| *s += stake)
                .or_insert(stake)
        });
    }

    fn sub_node_stake(&mut self, stake: u64, vote_account: &VoteAccount) {
        let Some(staked_nodes) = self.staked_nodes.get_mut() else {
            return;
        };

        VoteAccounts::do_sub_node_stake(staked_nodes, stake, vote_account.node_pubkey());
    }

    fn do_sub_node_stake(
        staked_nodes: &mut Arc<HashMap<Pubkey, u64>>,
        stake: u64,
        node_pubkey: Option<&Pubkey>,
    ) {
        if stake == 0u64 {
            return;
        }

        if let Some(node_pubkey) = node_pubkey {
            let staked_nodes = Arc::make_mut(staked_nodes);
            let current_stake = staked_nodes
                .get_mut(node_pubkey)
                .expect("this should not happen");
            match (*current_stake).cmp(&stake) {
                Ordering::Less => panic!("subtraction value exceeds node's stake"),
                Ordering::Equal => {
                    staked_nodes.remove(node_pubkey);
                }
                Ordering::Greater => *current_stake -= stake,
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

impl From<VoteAccount> for AccountSharedData {
    fn from(account: VoteAccount) -> Self {
        account.0.account.clone()
    }
}

impl TryFrom<AccountSharedData> for VoteAccount {
    type Error = Error;
    fn try_from(account: AccountSharedData) -> Result<Self, Self::Error> {
        let vote_account = VoteAccountInner::try_from(account)?;
        Ok(Self(Arc::new(vote_account)))
    }
}

impl TryFrom<AccountSharedData> for VoteAccountInner {
    type Error = Error;
    fn try_from(account: AccountSharedData) -> Result<Self, Self::Error> {
        if !solana_sdk::vote::program::check_id(account.owner()) {
            return Err(Error::InvalidOwner(*account.owner()));
        }
        Ok(Self {
            account,
            vote_state: OnceLock::new(),
        })
    }
}

impl PartialEq<VoteAccountInner> for VoteAccountInner {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            account,
            vote_state: _,
        } = self;
        account == &other.account
    }
}

impl Default for VoteAccounts {
    fn default() -> Self {
        Self {
            vote_accounts: Arc::default(),
            staked_nodes: OnceLock::new(),
        }
    }
}

impl PartialEq<VoteAccounts> for VoteAccounts {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            vote_accounts,
            staked_nodes: _,
        } = self;
        vote_accounts == &other.vote_accounts
    }
}

impl From<Arc<VoteAccountsHashMap>> for VoteAccounts {
    fn from(vote_accounts: Arc<VoteAccountsHashMap>) -> Self {
        Self {
            vote_accounts,
            staked_nodes: OnceLock::new(),
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
        solana_sdk::{
            pubkey::Pubkey,
            sysvar::clock::Clock,
            vote::state::{VoteInit, VoteStateVersions},
        },
        std::iter::repeat_with,
    };

    fn new_rand_vote_account<R: Rng>(
        rng: &mut R,
        node_pubkey: Option<Pubkey>,
    ) -> (AccountSharedData, VoteState) {
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
        let account = AccountSharedData::new_data(
            rng.gen(), // lamports
            &VoteStateVersions::new_current(vote_state.clone()),
            &solana_sdk::vote::program::id(), // owner
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
            let node = nodes[rng.gen_range(0..nodes.len())];
            let (account, _) = new_rand_vote_account(rng, Some(node));
            let stake = rng.gen_range(0..997);
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
                    .entry(*node_pubkey)
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
        assert_eq!(vote_state, *vote_account.vote_state().unwrap());
        // 2nd call to .vote_state() should return the cached value.
        assert_eq!(vote_state, *vote_account.vote_state().unwrap());
    }

    #[test]
    fn test_vote_account_serialize() {
        let mut rng = rand::thread_rng();
        let (account, vote_state) = new_rand_vote_account(&mut rng, None);
        let vote_account = VoteAccount::try_from(account.clone()).unwrap();
        assert_eq!(vote_state, *vote_account.vote_state().unwrap());
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
        assert_eq!(vote_state, *vote_account.vote_state().unwrap());
        let other_vote_account: VoteAccount = bincode::deserialize(&data).unwrap();
        assert_eq!(vote_account, other_vote_account);
        assert_eq!(vote_state, *other_vote_account.vote_state().unwrap());
    }

    #[test]
    fn test_vote_account_round_trip() {
        let mut rng = rand::thread_rng();
        let (account, vote_state) = new_rand_vote_account(&mut rng, None);
        let vote_account = VoteAccount::try_from(account).unwrap();
        assert_eq!(vote_state, *vote_account.vote_state().unwrap());
        let data = bincode::serialize(&vote_account).unwrap();
        let other_vote_account: VoteAccount = bincode::deserialize(&data).unwrap();
        // Assert that serialize->deserialized returns the same VoteAccount.
        assert_eq!(vote_account, other_vote_account);
        assert_eq!(vote_state, *other_vote_account.vote_state().unwrap());
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
            vote_accounts.insert(*pubkey, vote_account.clone(), || *stake);
            if (k + 1) % 128 == 0 {
                assert_eq!(
                    staked_nodes(&accounts[..k + 1]),
                    *vote_accounts.staked_nodes()
                );
            }
        }
        // Remove some of the vote accounts.
        for k in 0..256 {
            let index = rng.gen_range(0..accounts.len());
            let (pubkey, (_, _)) = accounts.swap_remove(index);
            vote_accounts.remove(&pubkey);
            if (k + 1) % 32 == 0 {
                assert_eq!(staked_nodes(&accounts), *vote_accounts.staked_nodes());
            }
        }
        // Modify the stakes for some of the accounts.
        for k in 0..2048 {
            let index = rng.gen_range(0..accounts.len());
            let (pubkey, (stake, _)) = &mut accounts[index];
            let new_stake = rng.gen_range(0..997);
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
            let index = rng.gen_range(0..accounts.len());
            let (pubkey, (_, _)) = accounts.swap_remove(index);
            vote_accounts.remove(&pubkey);
            if accounts.len() % 32 == 0 {
                assert_eq!(staked_nodes(&accounts), *vote_accounts.staked_nodes());
            }
        }
        assert!(vote_accounts.staked_nodes.get().unwrap().is_empty());
    }

    #[test]
    fn test_staked_nodes_update() {
        let mut vote_accounts = VoteAccounts::default();

        let mut rng = rand::thread_rng();
        let pubkey = Pubkey::new_unique();
        let node_pubkey = Pubkey::new_unique();
        let (account1, _) = new_rand_vote_account(&mut rng, Some(node_pubkey));
        let vote_account1 = VoteAccount::try_from(account1).unwrap();

        // first insert
        let ret = vote_accounts.insert(pubkey, vote_account1.clone(), || 42);
        assert_eq!(ret, None);
        assert_eq!(vote_accounts.get_delegated_stake(&pubkey), 42);
        assert_eq!(vote_accounts.staked_nodes().get(&node_pubkey), Some(&42));

        // update with unchanged state
        let ret = vote_accounts.insert(pubkey, vote_account1.clone(), || {
            panic!("should not be called")
        });
        assert_eq!(ret, Some(vote_account1.clone()));
        assert_eq!(vote_accounts.get(&pubkey), Some(&vote_account1));
        // stake is unchanged
        assert_eq!(vote_accounts.get_delegated_stake(&pubkey), 42);
        assert_eq!(vote_accounts.staked_nodes().get(&node_pubkey), Some(&42));

        // update with changed state, same node pubkey
        let (account2, _) = new_rand_vote_account(&mut rng, Some(node_pubkey));
        let vote_account2 = VoteAccount::try_from(account2).unwrap();
        let ret = vote_accounts.insert(pubkey, vote_account2.clone(), || {
            panic!("should not be called")
        });
        assert_eq!(ret, Some(vote_account1.clone()));
        assert_eq!(vote_accounts.get(&pubkey), Some(&vote_account2));
        // stake is unchanged
        assert_eq!(vote_accounts.get_delegated_stake(&pubkey), 42);
        assert_eq!(vote_accounts.staked_nodes().get(&node_pubkey), Some(&42));

        // update with new node pubkey, stake must be moved
        let new_node_pubkey = Pubkey::new_unique();
        let (account3, _) = new_rand_vote_account(&mut rng, Some(new_node_pubkey));
        let vote_account3 = VoteAccount::try_from(account3).unwrap();
        let ret = vote_accounts.insert(pubkey, vote_account3.clone(), || {
            panic!("should not be called")
        });
        assert_eq!(ret, Some(vote_account2.clone()));
        assert_eq!(vote_accounts.staked_nodes().get(&node_pubkey), None);
        assert_eq!(
            vote_accounts.staked_nodes().get(&new_node_pubkey),
            Some(&42)
        );
    }

    #[test]
    fn test_staked_nodes_zero_stake() {
        let mut vote_accounts = VoteAccounts::default();

        let mut rng = rand::thread_rng();
        let pubkey = Pubkey::new_unique();
        let node_pubkey = Pubkey::new_unique();
        let (account1, _) = new_rand_vote_account(&mut rng, Some(node_pubkey));
        let vote_account1 = VoteAccount::try_from(account1).unwrap();

        // we call this here to initialize VoteAccounts::staked_nodes which is a OnceLock
        assert!(vote_accounts.staked_nodes().is_empty());
        let ret = vote_accounts.insert(pubkey, vote_account1.clone(), || 0);
        assert_eq!(ret, None);
        assert_eq!(vote_accounts.get_delegated_stake(&pubkey), 0);
        // ensure that we didn't add a 0 stake entry to staked_nodes
        assert_eq!(vote_accounts.staked_nodes().get(&node_pubkey), None);

        // update with new node pubkey, stake is 0 and should remain 0
        let new_node_pubkey = Pubkey::new_unique();
        let (account2, _) = new_rand_vote_account(&mut rng, Some(new_node_pubkey));
        let vote_account2 = VoteAccount::try_from(account2).unwrap();
        let ret = vote_accounts.insert(pubkey, vote_account2.clone(), || {
            panic!("should not be called")
        });
        assert_eq!(ret, Some(vote_account1));
        assert_eq!(vote_accounts.get_delegated_stake(&pubkey), 0);
        assert_eq!(vote_accounts.staked_nodes().get(&node_pubkey), None);
        assert_eq!(vote_accounts.staked_nodes().get(&new_node_pubkey), None);
    }

    // Asserts that returned staked-nodes are copy-on-write references.
    #[test]
    fn test_staked_nodes_cow() {
        let mut rng = rand::thread_rng();
        let mut accounts = new_rand_vote_accounts(&mut rng, 64);
        // Add vote accounts.
        let mut vote_accounts = VoteAccounts::default();
        for (pubkey, (stake, vote_account)) in (&mut accounts).take(1024) {
            vote_accounts.insert(pubkey, vote_account, || stake);
        }
        let staked_nodes = vote_accounts.staked_nodes();
        let (pubkey, (more_stake, vote_account)) =
            accounts.find(|(_, (stake, _))| *stake != 0).unwrap();
        let node_pubkey = *vote_account.node_pubkey().unwrap();
        vote_accounts.insert(pubkey, vote_account, || more_stake);
        assert_ne!(staked_nodes, vote_accounts.staked_nodes());
        assert_eq!(
            vote_accounts.staked_nodes()[&node_pubkey],
            more_stake + staked_nodes.get(&node_pubkey).copied().unwrap_or_default()
        );
        for (pubkey, stake) in vote_accounts.staked_nodes().iter() {
            if pubkey != &node_pubkey {
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
            vote_accounts.insert(pubkey, vote_account, || stake);
        }
        let vote_accounts_hashmap = Arc::<VoteAccountsHashMap>::from(&vote_accounts);
        assert_eq!(vote_accounts_hashmap, vote_accounts.vote_accounts);
        assert!(Arc::ptr_eq(
            &vote_accounts_hashmap,
            &vote_accounts.vote_accounts
        ));
        let (pubkey, (more_stake, vote_account)) =
            accounts.find(|(_, (stake, _))| *stake != 0).unwrap();
        vote_accounts.insert(pubkey, vote_account.clone(), || more_stake);
        assert!(!Arc::ptr_eq(
            &vote_accounts_hashmap,
            &vote_accounts.vote_accounts
        ));
        assert_ne!(vote_accounts_hashmap, vote_accounts.vote_accounts);
        let other = (more_stake, vote_account);
        for (pk, value) in vote_accounts.vote_accounts.iter() {
            if *pk != pubkey {
                assert_eq!(value, &vote_accounts_hashmap[pk]);
            } else {
                assert_eq!(value, &other);
            }
        }
    }
}

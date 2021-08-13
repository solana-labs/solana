use {
    solana_account_decoder::{UiAccountEncoding, UiDataSliceConfig},
    solana_client::rpc_filter::RpcFilterType,
    solana_runtime::{
        bank::{TransactionLogCollectorConfig, TransactionLogCollectorFilter},
        bank_forks::BankForks,
    },
    solana_sdk::{
        clock::Slot, commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Signature,
    },
    std::{
        collections::{
            hash_map::{Entry, HashMap},
            HashSet,
        },
        fmt,
        sync::{Arc, Mutex, RwLock, Weak},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubscriptionId(u64);

impl From<u64> for SubscriptionId {
    fn from(value: u64) -> Self {
        SubscriptionId(value)
    }
}

impl From<SubscriptionId> for u64 {
    fn from(value: SubscriptionId) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubscriptionParams {
    Account(AccountSubscriptionParams),
    Logs(LogsSubscriptionParams),
    Program(ProgramSubscriptionParams),
    Signature(SignatureSubscriptionParams),
    Slot,
    SlotsUpdates,
    Root,
    Vote,
}

impl SubscriptionParams {
    fn method(&self) -> &'static str {
        match self {
            SubscriptionParams::Account(_) => "accountNotification",
            SubscriptionParams::Logs(_) => "logsNotification",
            SubscriptionParams::Program(_) => "programNotification",
            SubscriptionParams::Signature(_) => "signatureNotification",
            SubscriptionParams::Slot => "slotNotification",
            SubscriptionParams::SlotsUpdates => "slotsUpdatesNotification",
            SubscriptionParams::Root => "rootNotification",
            SubscriptionParams::Vote => "voteNotification",
        }
    }

    fn commitment(&self) -> Option<CommitmentConfig> {
        match self {
            SubscriptionParams::Account(params) => Some(params.commitment),
            SubscriptionParams::Logs(params) => Some(params.commitment),
            SubscriptionParams::Program(params) => Some(params.commitment),
            SubscriptionParams::Signature(params) => Some(params.commitment),
            SubscriptionParams::Slot
            | SubscriptionParams::SlotsUpdates
            | SubscriptionParams::Root
            | SubscriptionParams::Vote => None,
        }
    }

    fn is_commitment_watcher(&self) -> bool {
        let commitment = match self {
            SubscriptionParams::Account(params) => &params.commitment,
            SubscriptionParams::Logs(params) => &params.commitment,
            SubscriptionParams::Program(params) => &params.commitment,
            SubscriptionParams::Signature(params) => &params.commitment,
            SubscriptionParams::Slot
            | SubscriptionParams::SlotsUpdates
            | SubscriptionParams::Root
            | SubscriptionParams::Vote => return false,
        };
        !commitment.is_confirmed()
    }

    fn is_gossip_watcher(&self) -> bool {
        let commitment = match self {
            SubscriptionParams::Account(params) => &params.commitment,
            SubscriptionParams::Logs(params) => &params.commitment,
            SubscriptionParams::Program(params) => &params.commitment,
            SubscriptionParams::Signature(params) => &params.commitment,
            SubscriptionParams::Slot
            | SubscriptionParams::SlotsUpdates
            | SubscriptionParams::Root
            | SubscriptionParams::Vote => return false,
        };
        commitment.is_confirmed()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountSubscriptionParams {
    pub pubkey: Pubkey,
    pub encoding: UiAccountEncoding,
    pub data_slice: Option<UiDataSliceConfig>,
    pub commitment: CommitmentConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LogsSubscriptionParams {
    pub kind: LogsSubscriptionKind,
    pub commitment: CommitmentConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LogsSubscriptionKind {
    All,
    AllWithVotes,
    Single(Pubkey),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProgramSubscriptionParams {
    pub pubkey: Pubkey,
    pub filters: Vec<RpcFilterType>,
    pub encoding: UiAccountEncoding,
    pub data_slice: Option<UiDataSliceConfig>,
    pub commitment: CommitmentConfig,
    pub with_context: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SignatureSubscriptionParams {
    pub signature: Signature,
    pub commitment: CommitmentConfig,
    pub enable_received_notification: bool,
}

#[derive(Debug)]
pub struct SubscriptionInfo {
    id: SubscriptionId,
    params: SubscriptionParams,
    method: &'static str,
    token: Weak<SubscriptionTokenInner>,
    pub last_notified_slot: RwLock<Slot>,
    commitment: Option<CommitmentConfig>,
}

impl SubscriptionInfo {
    pub fn id(&self) -> SubscriptionId {
        self.id
    }

    pub fn method(&self) -> &'static str {
        self.method
    }

    pub fn params(&self) -> &SubscriptionParams {
        &self.params
    }

    pub fn commitment(&self) -> Option<CommitmentConfig> {
        self.commitment
    }

    pub fn num_subscribers(&self) -> usize {
        self.token.strong_count()
    }
}

struct LogsSubscriptionsIndex {
    all_count: usize,
    all_with_votes_count: usize,
    single_count: HashMap<Pubkey, usize>,

    bank_forks: Arc<RwLock<BankForks>>,
}

impl LogsSubscriptionsIndex {
    fn add(&mut self, params: &LogsSubscriptionParams) {
        match params.kind {
            LogsSubscriptionKind::All => self.all_count += 1,
            LogsSubscriptionKind::AllWithVotes => self.all_with_votes_count += 1,
            LogsSubscriptionKind::Single(key) => {
                *self.single_count.entry(key).or_default() += 1;
            }
        }
        self.update_config();
    }

    fn remove(&mut self, params: &LogsSubscriptionParams) {
        match params.kind {
            LogsSubscriptionKind::All => self.all_count -= 1,
            LogsSubscriptionKind::AllWithVotes => self.all_with_votes_count -= 1,
            LogsSubscriptionKind::Single(key) => match self.single_count.entry(key) {
                Entry::Occupied(mut entry) => {
                    *entry.get_mut() -= 1;
                    if *entry.get() == 0 {
                        entry.remove();
                    }
                }
                Entry::Vacant(_) => error!("missing entry in single_count"),
            },
        }
        self.update_config();
    }

    fn update_config(&self) {
        let config = if self.all_with_votes_count > 0 {
            TransactionLogCollectorConfig {
                filter: TransactionLogCollectorFilter::AllWithVotes,
                mentioned_addresses: HashSet::new(),
            }
        } else if self.all_count > 0 {
            TransactionLogCollectorConfig {
                filter: TransactionLogCollectorFilter::All,
                mentioned_addresses: HashSet::new(),
            }
        } else {
            TransactionLogCollectorConfig {
                filter: TransactionLogCollectorFilter::OnlyMentionedAddresses,
                mentioned_addresses: self.single_count.keys().copied().collect(),
            }
        };

        *self
            .bank_forks
            .read()
            .unwrap()
            .root_bank()
            .transaction_log_collector_config
            .write()
            .unwrap() = config;
    }
}

struct SubscriptionsInner {
    by_id: HashMap<SubscriptionId, Arc<SubscriptionInfo>>,
    next_id: u64,

    // Below are indexes for fast access.
    logs_subscriptions_index: LogsSubscriptionsIndex,
    by_params: HashMap<SubscriptionParams, Arc<SubscriptionInfo>>,
    by_signature: HashMap<Signature, HashMap<SubscriptionId, Arc<SubscriptionInfo>>>,
    // Accounts, logs, programs, signatures (not gossip)
    commitment_watchers: HashMap<SubscriptionId, Arc<SubscriptionInfo>>,
    // Accounts, logs, programs, signatures (gossip)
    gossip_watchers: HashMap<SubscriptionId, Arc<SubscriptionInfo>>,
}

#[derive(Clone)]
pub struct SubscriptionsTracker(Arc<Mutex<SubscriptionsInner>>);

impl SubscriptionsTracker {
    pub fn new(bank_forks: Arc<RwLock<BankForks>>) -> Self {
        SubscriptionsTracker(Arc::new(Mutex::new(SubscriptionsInner {
            by_id: HashMap::new(),
            by_params: HashMap::new(),
            next_id: 0,
            logs_subscriptions_index: LogsSubscriptionsIndex {
                all_count: 0,
                all_with_votes_count: 0,
                single_count: HashMap::new(),
                bank_forks,
            },
            by_signature: HashMap::new(),
            commitment_watchers: HashMap::new(),
            gossip_watchers: HashMap::new(),
        })))
    }

    pub fn total(&self) -> usize {
        let inner = self.0.lock().unwrap();
        inner.by_id.len()
    }

    pub fn subscribe(
        &self,
        params: SubscriptionParams,
        last_notified_slot: impl FnOnce() -> Slot,
    ) -> SubscriptionToken {
        let mut inner = &mut *self.0.lock().unwrap();
        match inner.by_params.entry(params) {
            Entry::Occupied(entry) => SubscriptionToken(
                entry
                    .get()
                    .token
                    .upgrade()
                    .expect("dead subscription encountered in by_params"),
            ),
            Entry::Vacant(entry) => {
                let id = SubscriptionId::from(inner.next_id);
                inner.next_id += 1;
                let token = SubscriptionToken(Arc::new(SubscriptionTokenInner {
                    subscriptions: Arc::clone(&self.0),
                    id,
                }));
                let params = entry.key().clone();
                let info = Arc::new(SubscriptionInfo {
                    token: Arc::downgrade(&token.0),
                    last_notified_slot: RwLock::new(last_notified_slot()),
                    id,
                    commitment: params.commitment(),
                    method: params.method(),
                    params: params.clone(),
                });
                entry.insert(Arc::clone(&info));
                match &params {
                    SubscriptionParams::Logs(params) => {
                        inner.logs_subscriptions_index.add(params);
                    }
                    SubscriptionParams::Signature(params) => {
                        inner
                            .by_signature
                            .entry(params.signature)
                            .or_default()
                            .insert(id, Arc::clone(&info));
                    }
                    _ => {}
                }
                if info.params.is_commitment_watcher() {
                    inner.commitment_watchers.insert(id, Arc::clone(&info));
                }
                if info.params.is_gossip_watcher() {
                    inner.gossip_watchers.insert(id, Arc::clone(&info));
                }
                inner.by_id.insert(id, info);
                token
            }
        }
    }

    pub fn visit_by_params<F, T>(&self, params: &SubscriptionParams, f: F) -> Option<T>
    where
        F: FnOnce(&SubscriptionInfo) -> T,
    {
        let inner = self.0.lock().unwrap();
        inner.by_params.get(params).map(|info| f(info))
    }

    pub fn visit_by_signature<F, T>(&self, signature: &Signature, f: F) -> Option<T>
    where
        F: FnOnce(&HashMap<SubscriptionId, Arc<SubscriptionInfo>>) -> T,
    {
        let inner = self.0.lock().unwrap();
        inner.by_signature.get(signature).map(f)
    }

    pub fn visit_commitment_watchers<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&HashMap<SubscriptionId, Arc<SubscriptionInfo>>) -> T,
    {
        let inner = self.0.lock().unwrap();
        f(&inner.commitment_watchers)
    }

    pub fn visit_gossip_watchers<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&HashMap<SubscriptionId, Arc<SubscriptionInfo>>) -> T,
    {
        let inner = self.0.lock().unwrap();
        f(&inner.gossip_watchers)
    }
}

struct SubscriptionTokenInner {
    subscriptions: Arc<Mutex<SubscriptionsInner>>,
    id: SubscriptionId,
}

impl fmt::Debug for SubscriptionTokenInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubscriptionTokenInner")
            .field("id", &self.id)
            .finish()
    }
}

impl Drop for SubscriptionTokenInner {
    #[allow(clippy::collapsible_if)]
    fn drop(&mut self) {
        if let Ok(mut inner) = self.subscriptions.lock() {
            if let Some(info) = inner.by_id.remove(&self.id) {
                if inner.by_params.remove(&info.params).is_none() {
                    warn!("Subscriptions inconsistency (missing entry in by_params)");
                }
                match &info.params {
                    SubscriptionParams::Logs(params) => {
                        inner.logs_subscriptions_index.remove(params);
                    }
                    SubscriptionParams::Signature(params) => {
                        if let Entry::Occupied(mut entry) =
                            inner.by_signature.entry(params.signature)
                        {
                            if entry.get_mut().remove(&self.id).is_none() {
                                warn!(
                                    "Subscriptions inconsistency (missing entry in by_signature)"
                                );
                            }
                            if entry.get_mut().is_empty() {
                                entry.remove();
                            }
                        } else {
                            warn!("Subscriptions inconsistency (missing entry in by_signature)");
                        }
                    }
                    _ => {}
                }
                if info.params.is_commitment_watcher() {
                    if inner.commitment_watchers.remove(&self.id).is_none() {
                        warn!("Subscriptions inconsistency (missing entry in commitment_watchers)");
                    }
                }
                if info.params.is_gossip_watcher() {
                    if inner.gossip_watchers.remove(&self.id).is_none() {
                        warn!("Subscriptions inconsistency (missing entry in gossip_watchers)");
                    }
                }
            } else {
                warn!("Subscriptions inconsistency (missing entry in by_id)");
            }
        } else {
            warn!("cannot unsubscribe: mutex is poisoned");
        }
    }
}

#[derive(Clone)]
pub struct SubscriptionToken(Arc<SubscriptionTokenInner>);

impl SubscriptionToken {
    pub fn id(&self) -> SubscriptionId {
        self.0.id
    }
}

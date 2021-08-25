use {
    crate::rpc_subscriptions::{NotificationEntry, RpcNotification},
    dashmap::{mapref::entry::Entry as DashEntry, DashMap},
    solana_account_decoder::{UiAccountEncoding, UiDataSliceConfig},
    solana_client::rpc_filter::RpcFilterType,
    solana_metrics::{CounterToken, TokenCounter},
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
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, RwLock, Weak,
        },
    },
    thiserror::Error,
    tokio::sync::broadcast,
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

#[derive(Clone)]
pub struct SubscriptionControl(Arc<SubscriptionControlInner>);

struct SubscriptionControlInner {
    subscriptions: DashMap<SubscriptionParams, Weak<SubscriptionTokenInner>>,
    next_id: AtomicU64,
    max_active_subscriptions: usize,
    sender: crossbeam_channel::Sender<NotificationEntry>,
    broadcast_sender: broadcast::Sender<RpcNotification>,
    counter: TokenCounter,
}

impl SubscriptionControl {
    pub fn new(
        max_active_subscriptions: usize,
        sender: crossbeam_channel::Sender<NotificationEntry>,
        broadcast_sender: broadcast::Sender<RpcNotification>,
    ) -> Self {
        Self(Arc::new(SubscriptionControlInner {
            subscriptions: DashMap::new(),
            next_id: AtomicU64::new(0),
            max_active_subscriptions,
            sender,
            broadcast_sender,
            counter: TokenCounter::new("rpc_pubsub_total_subscriptions"),
        }))
    }

    pub fn broadcast_receiver(&self) -> broadcast::Receiver<RpcNotification> {
        self.0.broadcast_sender.subscribe()
    }

    pub fn subscribe(
        &self,
        params: SubscriptionParams,
    ) -> Result<SubscriptionToken, TooManySubscriptions> {
        debug!(
            "Total existing subscriptions: {}",
            self.0.subscriptions.len()
        );
        let count = self.0.subscriptions.len();
        match self.0.subscriptions.entry(params) {
            DashEntry::Occupied(entry) => Ok(SubscriptionToken(
                entry
                    .get()
                    .upgrade()
                    .expect("dead subscription encountered in SubscriptionControl"),
                self.0.counter.create_token(),
            )),
            DashEntry::Vacant(entry) => {
                if count >= self.0.max_active_subscriptions {
                    inc_new_counter_info!("rpc-subscription-refused-limit-reached", 1);
                    return Err(TooManySubscriptions(()));
                }
                let id = SubscriptionId::from(self.0.next_id.fetch_add(1, Ordering::AcqRel));
                let token = SubscriptionToken(
                    Arc::new(SubscriptionTokenInner {
                        control: Arc::clone(&self.0),
                        params: entry.key().clone(),
                        id,
                    }),
                    self.0.counter.create_token(),
                );
                let _ = self
                    .0
                    .sender
                    .send(NotificationEntry::Subscribed(token.0.params.clone(), id));
                entry.insert(Arc::downgrade(&token.0));
                datapoint_info!(
                    "rpc-subscription",
                    ("total", self.0.subscriptions.len(), i64)
                );
                Ok(token)
            }
        }
    }

    pub fn total(&self) -> usize {
        self.0.subscriptions.len()
    }

    #[cfg(test)]
    pub fn assert_subscribed(&self, params: &SubscriptionParams) {
        assert!(self.0.subscriptions.contains_key(params));
    }

    #[cfg(test)]
    pub fn assert_unsubscribed(&self, params: &SubscriptionParams) {
        assert!(!self.0.subscriptions.contains_key(params));
    }

    #[cfg(test)]
    pub fn account_subscribed(&self, pubkey: &Pubkey) -> bool {
        self.0.subscriptions.iter().any(|item| {
            if let SubscriptionParams::Account(params) = item.key() {
                &params.pubkey == pubkey
            } else {
                false
            }
        })
    }

    #[cfg(test)]
    pub fn signature_subscribed(&self, signature: &Signature) -> bool {
        self.0.subscriptions.iter().any(|item| {
            if let SubscriptionParams::Signature(params) = item.key() {
                &params.signature == signature
            } else {
                false
            }
        })
    }
}

#[derive(Debug)]
pub struct SubscriptionInfo {
    id: SubscriptionId,
    params: SubscriptionParams,
    method: &'static str,
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
}

#[derive(Debug, Error)]
#[error("node subscription limit reached")]
pub struct TooManySubscriptions(());

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

pub struct SubscriptionsTracker {
    logs_subscriptions_index: LogsSubscriptionsIndex,
    by_params: HashMap<SubscriptionParams, Arc<SubscriptionInfo>>,
    by_signature: HashMap<Signature, HashMap<SubscriptionId, Arc<SubscriptionInfo>>>,
    // Accounts, logs, programs, signatures (not gossip)
    commitment_watchers: HashMap<SubscriptionId, Arc<SubscriptionInfo>>,
    // Accounts, logs, programs, signatures (gossip)
    gossip_watchers: HashMap<SubscriptionId, Arc<SubscriptionInfo>>,
}

impl SubscriptionsTracker {
    pub fn new(bank_forks: Arc<RwLock<BankForks>>) -> Self {
        SubscriptionsTracker {
            by_params: HashMap::new(),
            logs_subscriptions_index: LogsSubscriptionsIndex {
                all_count: 0,
                all_with_votes_count: 0,
                single_count: HashMap::new(),
                bank_forks,
            },
            by_signature: HashMap::new(),
            commitment_watchers: HashMap::new(),
            gossip_watchers: HashMap::new(),
        }
    }

    pub fn total(&self) -> usize {
        self.by_params.len()
    }

    pub fn subscribe(
        &mut self,
        params: SubscriptionParams,
        id: SubscriptionId,
        last_notified_slot: impl FnOnce() -> Slot,
    ) {
        let info = Arc::new(SubscriptionInfo {
            last_notified_slot: RwLock::new(last_notified_slot()),
            id,
            commitment: params.commitment(),
            method: params.method(),
            params: params.clone(),
        });
        self.by_params.insert(params.clone(), Arc::clone(&info));
        match &params {
            SubscriptionParams::Logs(params) => {
                self.logs_subscriptions_index.add(params);
            }
            SubscriptionParams::Signature(params) => {
                self.by_signature
                    .entry(params.signature)
                    .or_default()
                    .insert(id, Arc::clone(&info));
            }
            _ => {}
        }
        if info.params.is_commitment_watcher() {
            self.commitment_watchers.insert(id, Arc::clone(&info));
        }
        if info.params.is_gossip_watcher() {
            self.gossip_watchers.insert(id, Arc::clone(&info));
        }
    }

    #[allow(clippy::collapsible_if)]
    pub fn unsubscribe(&mut self, params: SubscriptionParams) {
        let info = if let Some(info) = self.by_params.remove(&params) {
            info
        } else {
            warn!("missing entry in SubscriptionTracker");
            return;
        };
        match &params {
            SubscriptionParams::Logs(params) => {
                self.logs_subscriptions_index.remove(params);
            }
            SubscriptionParams::Signature(params) => {
                if let Entry::Occupied(mut entry) = self.by_signature.entry(params.signature) {
                    if entry.get_mut().remove(&info.id).is_none() {
                        warn!("Subscriptions inconsistency (missing entry in by_signature)");
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
        if params.is_commitment_watcher() {
            if self.commitment_watchers.remove(&info.id).is_none() {
                warn!("Subscriptions inconsistency (missing entry in commitment_watchers)");
            }
        }
        if params.is_gossip_watcher() {
            if self.gossip_watchers.remove(&info.id).is_none() {
                warn!("Subscriptions inconsistency (missing entry in gossip_watchers)");
            }
        }
    }

    pub fn by_params(&self) -> &HashMap<SubscriptionParams, Arc<SubscriptionInfo>> {
        &self.by_params
    }
    pub fn by_signature(
        &self,
    ) -> &HashMap<Signature, HashMap<SubscriptionId, Arc<SubscriptionInfo>>> {
        &self.by_signature
    }
    pub fn commitment_watchers(&self) -> &HashMap<SubscriptionId, Arc<SubscriptionInfo>> {
        &self.commitment_watchers
    }
    pub fn gossip_watchers(&self) -> &HashMap<SubscriptionId, Arc<SubscriptionInfo>> {
        &self.gossip_watchers
    }
}

struct SubscriptionTokenInner {
    control: Arc<SubscriptionControlInner>,
    params: SubscriptionParams,
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
        match self.control.subscriptions.entry(self.params.clone()) {
            DashEntry::Vacant(_) => {
                warn!("Subscriptions inconsistency (missing entry in by_params)");
            }
            DashEntry::Occupied(entry) => {
                let _ = self
                    .control
                    .sender
                    .send(NotificationEntry::Unsubscribed(self.params.clone()));
                entry.remove();
                datapoint_info!(
                    "rpc-subscription",
                    ("total", self.control.subscriptions.len(), i64)
                );
            }
        }
    }
}

#[derive(Clone)]
pub struct SubscriptionToken(Arc<SubscriptionTokenInner>, CounterToken);

impl SubscriptionToken {
    pub fn id(&self) -> SubscriptionId {
        self.0.id
    }

    pub fn params(&self) -> &SubscriptionParams {
        &self.0.params
    }
}

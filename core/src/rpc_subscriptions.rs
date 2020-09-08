//! The `pubsub` module implements a threaded subscription service on client RPC request

use crate::rpc::{get_parsed_token_account, get_parsed_token_accounts};
use core::hash::Hash;
use jsonrpc_core::futures::Future;
use jsonrpc_pubsub::{
    typed::{Sink, Subscriber},
    SubscriptionId,
};
use serde::Serialize;
use solana_account_decoder::{parse_token::spl_token_id_v2_0, UiAccount, UiAccountEncoding};
use solana_client::{
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcSignatureSubscribeConfig},
    rpc_filter::RpcFilterType,
    rpc_response::{
        ProcessedSignatureResult, ReceivedSignatureResult, Response, RpcKeyedAccount,
        RpcResponseContext, RpcSignatureResult, SlotInfo,
    },
};
use solana_runtime::{
    bank::Bank,
    bank_forks::BankForks,
    commitment::{BlockCommitmentCache, CommitmentSlots},
};
use solana_sdk::{
    account::Account,
    clock::{Slot, UnixTimestamp},
    commitment_config::{CommitmentConfig, CommitmentLevel},
    pubkey::Pubkey,
    signature::Signature,
    transaction,
};
use solana_vote_program::vote_state::Vote;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, RecvTimeoutError, SendError, Sender},
};
use std::thread::{Builder, JoinHandle};
use std::time::Duration;
use std::{
    collections::{HashMap, HashSet},
    iter,
    sync::{Arc, Mutex, RwLock},
};

// Stuck on tokio 0.1 until the jsonrpc-pubsub crate upgrades to tokio 0.2
use tokio_01::runtime::{Builder as RuntimeBuilder, Runtime, TaskExecutor};

const RECEIVE_DELAY_MILLIS: u64 = 100;

// A more human-friendly version of Vote, with the bank state signature base58 encoded.
#[derive(Serialize, Deserialize, Debug)]
pub struct RpcVote {
    pub slots: Vec<Slot>,
    pub hash: String,
    pub timestamp: Option<UnixTimestamp>,
}

enum NotificationEntry {
    Slot(SlotInfo),
    Vote(Vote),
    Root(Slot),
    Frozen(Slot),
    Bank(CommitmentSlots),
    Gossip(Slot),
    SignaturesReceived((Slot, Vec<Signature>)),
}

impl std::fmt::Debug for NotificationEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            NotificationEntry::Root(root) => write!(f, "Root({})", root),
            NotificationEntry::Frozen(slot) => write!(f, "Frozen({})", slot),
            NotificationEntry::Vote(vote) => write!(f, "Vote({:?})", vote),
            NotificationEntry::Slot(slot_info) => write!(f, "Slot({:?})", slot_info),
            NotificationEntry::Bank(commitment_slots) => {
                write!(f, "Bank({{slot: {:?}}})", commitment_slots.slot)
            }
            NotificationEntry::SignaturesReceived(slot_signatures) => {
                write!(f, "SignaturesReceived({:?})", slot_signatures)
            }
            NotificationEntry::Gossip(slot) => write!(f, "Gossip({:?})", slot),
        }
    }
}

struct SubscriptionData<S, T> {
    sink: Sink<S>,
    commitment: CommitmentConfig,
    last_notified_slot: RwLock<Slot>,
    config: Option<T>,
}
#[derive(Default, Clone)]
struct ProgramConfig {
    filters: Vec<RpcFilterType>,
    encoding: Option<UiAccountEncoding>,
}
type RpcAccountSubscriptions = RwLock<
    HashMap<
        Pubkey,
        HashMap<SubscriptionId, SubscriptionData<Response<UiAccount>, UiAccountEncoding>>,
    >,
>;
type RpcProgramSubscriptions = RwLock<
    HashMap<
        Pubkey,
        HashMap<SubscriptionId, SubscriptionData<Response<RpcKeyedAccount>, ProgramConfig>>,
    >,
>;
type RpcSignatureSubscriptions = RwLock<
    HashMap<
        Signature,
        HashMap<SubscriptionId, SubscriptionData<Response<RpcSignatureResult>, bool>>,
    >,
>;
type RpcSlotSubscriptions = RwLock<HashMap<SubscriptionId, Sink<SlotInfo>>>;
type RpcVoteSubscriptions = RwLock<HashMap<SubscriptionId, Sink<RpcVote>>>;
type RpcRootSubscriptions = RwLock<HashMap<SubscriptionId, Sink<Slot>>>;

fn add_subscription<K, S, T>(
    subscriptions: &mut HashMap<K, HashMap<SubscriptionId, SubscriptionData<S, T>>>,
    hashmap_key: K,
    commitment: Option<CommitmentConfig>,
    sub_id: SubscriptionId,
    subscriber: Subscriber<S>,
    last_notified_slot: Slot,
    config: Option<T>,
) where
    K: Eq + Hash,
    S: Clone,
{
    let sink = subscriber.assign_id(sub_id.clone()).unwrap();
    let commitment = commitment.unwrap_or_else(CommitmentConfig::single);
    let subscription_data = SubscriptionData {
        sink,
        commitment,
        last_notified_slot: RwLock::new(last_notified_slot),
        config,
    };

    subscriptions
        .entry(hashmap_key)
        .or_default()
        .insert(sub_id, subscription_data);
}

fn remove_subscription<K, S, T>(
    subscriptions: &mut HashMap<K, HashMap<SubscriptionId, SubscriptionData<S, T>>>,
    sub_id: &SubscriptionId,
) -> bool
where
    K: Eq + Hash,
    S: Clone,
{
    let mut found = false;
    subscriptions.retain(|_, v| {
        v.retain(|k, _| {
            let retain = k != sub_id;
            if !retain {
                found = true;
            }
            retain
        });
        !v.is_empty()
    });
    found
}

#[allow(clippy::type_complexity)]
fn check_commitment_and_notify<K, S, B, F, X, T>(
    subscriptions: &HashMap<K, HashMap<SubscriptionId, SubscriptionData<Response<S>, T>>>,
    hashmap_key: &K,
    bank_forks: &Arc<RwLock<BankForks>>,
    commitment_slots: &CommitmentSlots,
    bank_method: B,
    filter_results: F,
    notifier: &RpcNotifier,
) -> HashSet<SubscriptionId>
where
    K: Eq + Hash + Clone + Copy,
    S: Clone + Serialize,
    B: Fn(&Bank, &K) -> X,
    F: Fn(X, &K, Slot, Option<T>, Option<Arc<Bank>>) -> (Box<dyn Iterator<Item = S>>, Slot),
    X: Clone + Serialize + Default,
    T: Clone,
{
    let mut notified_set: HashSet<SubscriptionId> = HashSet::new();
    if let Some(hashmap) = subscriptions.get(hashmap_key) {
        for (
            sub_id,
            SubscriptionData {
                sink,
                commitment,
                last_notified_slot,
                config,
            },
        ) in hashmap.iter()
        {
            let slot = match commitment.commitment {
                CommitmentLevel::Max => commitment_slots.highest_confirmed_root,
                CommitmentLevel::Recent => commitment_slots.slot,
                CommitmentLevel::Root => commitment_slots.root,
                CommitmentLevel::Single | CommitmentLevel::SingleGossip => {
                    commitment_slots.highest_confirmed_slot
                }
            };
            let bank = bank_forks.read().unwrap().get(slot).cloned();
            let results = bank
                .clone()
                .map(|desired_bank| bank_method(&desired_bank, hashmap_key))
                .unwrap_or_default();
            let mut w_last_notified_slot = last_notified_slot.write().unwrap();
            let (filter_results, result_slot) = filter_results(
                results,
                hashmap_key,
                *w_last_notified_slot,
                config.as_ref().cloned(),
                bank,
            );
            for result in filter_results {
                notifier.notify(
                    Response {
                        context: RpcResponseContext { slot },
                        value: result,
                    },
                    sink,
                );
                notified_set.insert(sub_id.clone());
                *w_last_notified_slot = result_slot;
            }
        }
    }
    notified_set
}

struct RpcNotifier(TaskExecutor);

impl RpcNotifier {
    fn notify<T>(&self, value: T, sink: &Sink<T>)
    where
        T: serde::Serialize,
    {
        self.0
            .spawn(sink.notify(Ok(value)).map(|_| ()).map_err(|_| ()));
    }
}

fn filter_account_result(
    result: Option<(Account, Slot)>,
    pubkey: &Pubkey,
    last_notified_slot: Slot,
    encoding: Option<UiAccountEncoding>,
    bank: Option<Arc<Bank>>,
) -> (Box<dyn Iterator<Item = UiAccount>>, Slot) {
    if let Some((account, fork)) = result {
        // If fork < last_notified_slot this means that we last notified for a fork
        // and should notify that the account state has been reverted.
        if fork != last_notified_slot {
            let encoding = encoding.unwrap_or(UiAccountEncoding::Binary);
            if account.owner == spl_token_id_v2_0() && encoding == UiAccountEncoding::JsonParsed {
                let bank = bank.unwrap(); // If result.is_some(), bank must also be Some
                return (
                    Box::new(iter::once(get_parsed_token_account(bank, pubkey, account))),
                    fork,
                );
            } else {
                return (
                    Box::new(iter::once(UiAccount::encode(
                        pubkey, account, encoding, None, None,
                    ))),
                    fork,
                );
            }
        }
    }
    (Box::new(iter::empty()), last_notified_slot)
}

fn filter_signature_result(
    result: Option<transaction::Result<()>>,
    _signature: &Signature,
    last_notified_slot: Slot,
    _config: Option<bool>,
    _bank: Option<Arc<Bank>>,
) -> (Box<dyn Iterator<Item = RpcSignatureResult>>, Slot) {
    (
        Box::new(result.into_iter().map(|result| {
            RpcSignatureResult::ProcessedSignature(ProcessedSignatureResult { err: result.err() })
        })),
        last_notified_slot,
    )
}

fn filter_program_results(
    accounts: Vec<(Pubkey, Account)>,
    _program_id: &Pubkey,
    last_notified_slot: Slot,
    config: Option<ProgramConfig>,
    bank: Option<Arc<Bank>>,
) -> (Box<dyn Iterator<Item = RpcKeyedAccount>>, Slot) {
    let config = config.unwrap_or_default();
    let encoding = config.encoding.unwrap_or(UiAccountEncoding::Binary);
    let filters = config.filters;
    let keyed_accounts = accounts.into_iter().filter(move |(_, account)| {
        filters.iter().all(|filter_type| match filter_type {
            RpcFilterType::DataSize(size) => account.data.len() as u64 == *size,
            RpcFilterType::Memcmp(compare) => compare.bytes_match(&account.data),
        })
    });
    let accounts: Box<dyn Iterator<Item = RpcKeyedAccount>> =
        if encoding == UiAccountEncoding::JsonParsed {
            let bank = bank.unwrap(); // If !accounts.is_empty(), bank must be Some
            Box::new(get_parsed_token_accounts(bank, keyed_accounts))
        } else {
            Box::new(
                keyed_accounts.map(move |(pubkey, account)| RpcKeyedAccount {
                    pubkey: pubkey.to_string(),
                    account: UiAccount::encode(&pubkey, account, encoding.clone(), None, None),
                }),
            )
        };
    (accounts, last_notified_slot)
}

#[derive(Clone)]
struct Subscriptions {
    account_subscriptions: Arc<RpcAccountSubscriptions>,
    program_subscriptions: Arc<RpcProgramSubscriptions>,
    signature_subscriptions: Arc<RpcSignatureSubscriptions>,
    gossip_account_subscriptions: Arc<RpcAccountSubscriptions>,
    gossip_program_subscriptions: Arc<RpcProgramSubscriptions>,
    gossip_signature_subscriptions: Arc<RpcSignatureSubscriptions>,
    slot_subscriptions: Arc<RpcSlotSubscriptions>,
    vote_subscriptions: Arc<RpcVoteSubscriptions>,
    root_subscriptions: Arc<RpcRootSubscriptions>,
}

pub struct RpcSubscriptions {
    subscriptions: Subscriptions,
    notification_sender: Arc<Mutex<Sender<NotificationEntry>>>,
    t_cleanup: Option<JoinHandle<()>>,
    notifier_runtime: Option<Runtime>,
    bank_forks: Arc<RwLock<BankForks>>,
    block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    last_checked_slots: Arc<RwLock<HashMap<CommitmentLevel, Slot>>>,
    exit: Arc<AtomicBool>,
}

impl Drop for RpcSubscriptions {
    fn drop(&mut self) {
        self.shutdown().unwrap_or_else(|err| {
            warn!("RPC Notification - shutdown error: {:?}", err);
        });
    }
}

impl RpcSubscriptions {
    pub fn new(
        exit: &Arc<AtomicBool>,
        bank_forks: Arc<RwLock<BankForks>>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    ) -> Self {
        let (notification_sender, notification_receiver): (
            Sender<NotificationEntry>,
            Receiver<NotificationEntry>,
        ) = std::sync::mpsc::channel();

        let account_subscriptions = Arc::new(RpcAccountSubscriptions::default());
        let program_subscriptions = Arc::new(RpcProgramSubscriptions::default());
        let signature_subscriptions = Arc::new(RpcSignatureSubscriptions::default());
        let gossip_account_subscriptions = Arc::new(RpcAccountSubscriptions::default());
        let gossip_program_subscriptions = Arc::new(RpcProgramSubscriptions::default());
        let gossip_signature_subscriptions = Arc::new(RpcSignatureSubscriptions::default());
        let slot_subscriptions = Arc::new(RpcSlotSubscriptions::default());
        let vote_subscriptions = Arc::new(RpcVoteSubscriptions::default());
        let root_subscriptions = Arc::new(RpcRootSubscriptions::default());
        let notification_sender = Arc::new(Mutex::new(notification_sender));

        let _bank_forks = bank_forks.clone();
        let _block_commitment_cache = block_commitment_cache.clone();
        let exit_clone = exit.clone();
        let subscriptions = Subscriptions {
            account_subscriptions,
            program_subscriptions,
            signature_subscriptions,
            gossip_account_subscriptions,
            gossip_program_subscriptions,
            gossip_signature_subscriptions,
            slot_subscriptions,
            vote_subscriptions,
            root_subscriptions,
        };
        let _subscriptions = subscriptions.clone();

        let last_checked_slots = Arc::new(RwLock::new(HashMap::new()));
        let _last_checked_slots = last_checked_slots.clone();

        let notifier_runtime = RuntimeBuilder::new()
            .core_threads(1)
            .name_prefix("solana-rpc-notifier-")
            .build()
            .unwrap();

        let notifier = RpcNotifier(notifier_runtime.executor());
        let t_cleanup = Builder::new()
            .name("solana-rpc-notifications".to_string())
            .spawn(move || {
                Self::process_notifications(
                    exit_clone,
                    notifier,
                    notification_receiver,
                    _subscriptions,
                    _bank_forks,
                    _last_checked_slots,
                );
            })
            .unwrap();

        Self {
            subscriptions,
            notification_sender,
            notifier_runtime: Some(notifier_runtime),
            t_cleanup: Some(t_cleanup),
            bank_forks,
            block_commitment_cache,
            last_checked_slots,
            exit: exit.clone(),
        }
    }

    pub fn default_with_bank_forks(bank_forks: Arc<RwLock<BankForks>>) -> Self {
        Self::new(
            &Arc::new(AtomicBool::new(false)),
            bank_forks,
            Arc::new(RwLock::new(BlockCommitmentCache::default())),
        )
    }

    fn check_account(
        pubkey: &Pubkey,
        bank_forks: &Arc<RwLock<BankForks>>,
        account_subscriptions: Arc<RpcAccountSubscriptions>,
        notifier: &RpcNotifier,
        commitment_slots: &CommitmentSlots,
    ) {
        let subscriptions = account_subscriptions.read().unwrap();
        check_commitment_and_notify(
            &subscriptions,
            pubkey,
            bank_forks,
            commitment_slots,
            Bank::get_account_modified_slot,
            filter_account_result,
            notifier,
        );
    }

    fn check_program(
        program_id: &Pubkey,
        bank_forks: &Arc<RwLock<BankForks>>,
        program_subscriptions: Arc<RpcProgramSubscriptions>,
        notifier: &RpcNotifier,
        commitment_slots: &CommitmentSlots,
    ) {
        let subscriptions = program_subscriptions.read().unwrap();
        check_commitment_and_notify(
            &subscriptions,
            program_id,
            bank_forks,
            commitment_slots,
            Bank::get_program_accounts_modified_since_parent,
            filter_program_results,
            notifier,
        );
    }

    fn check_signature(
        signature: &Signature,
        bank_forks: &Arc<RwLock<BankForks>>,
        signature_subscriptions: Arc<RpcSignatureSubscriptions>,
        notifier: &RpcNotifier,
        commitment_slots: &CommitmentSlots,
    ) {
        let mut subscriptions = signature_subscriptions.write().unwrap();
        let notified_ids = check_commitment_and_notify(
            &subscriptions,
            signature,
            bank_forks,
            commitment_slots,
            Bank::get_signature_status_processed_since_parent,
            filter_signature_result,
            notifier,
        );
        if let Some(subscription_ids) = subscriptions.get_mut(signature) {
            subscription_ids.retain(|k, _| !notified_ids.contains(k));
            if subscription_ids.is_empty() {
                subscriptions.remove(&signature);
            }
        }
    }

    pub fn add_account_subscription(
        &self,
        pubkey: Pubkey,
        config: Option<RpcAccountInfoConfig>,
        sub_id: SubscriptionId,
        subscriber: Subscriber<Response<UiAccount>>,
    ) {
        let config = config.unwrap_or_default();
        let commitment_level = config
            .commitment
            .unwrap_or_else(CommitmentConfig::single)
            .commitment;
        let slot = match commitment_level {
            CommitmentLevel::Max => self
                .block_commitment_cache
                .read()
                .unwrap()
                .highest_confirmed_root(),
            CommitmentLevel::Recent => self.block_commitment_cache.read().unwrap().slot(),
            CommitmentLevel::Root => self.block_commitment_cache.read().unwrap().root(),
            CommitmentLevel::Single => self
                .block_commitment_cache
                .read()
                .unwrap()
                .highest_confirmed_slot(),
            CommitmentLevel::SingleGossip => *self
                .last_checked_slots
                .read()
                .unwrap()
                .get(&CommitmentLevel::SingleGossip)
                .unwrap_or(&0),
        };
        let last_notified_slot = if let Some((_account, slot)) = self
            .bank_forks
            .read()
            .unwrap()
            .get(slot)
            .and_then(|bank| bank.get_account_modified_slot(&pubkey))
        {
            slot
        } else {
            0
        };

        let mut subscriptions = if commitment_level == CommitmentLevel::SingleGossip {
            self.subscriptions
                .gossip_account_subscriptions
                .write()
                .unwrap()
        } else {
            self.subscriptions.account_subscriptions.write().unwrap()
        };
        add_subscription(
            &mut subscriptions,
            pubkey,
            config.commitment,
            sub_id,
            subscriber,
            last_notified_slot,
            config.encoding,
        );
    }

    pub fn remove_account_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.subscriptions.account_subscriptions.write().unwrap();
        if remove_subscription(&mut subscriptions, id) {
            true
        } else {
            let mut subscriptions = self
                .subscriptions
                .gossip_account_subscriptions
                .write()
                .unwrap();
            remove_subscription(&mut subscriptions, id)
        }
    }

    pub fn add_program_subscription(
        &self,
        program_id: Pubkey,
        config: Option<RpcProgramAccountsConfig>,
        sub_id: SubscriptionId,
        subscriber: Subscriber<Response<RpcKeyedAccount>>,
    ) {
        let config = config.unwrap_or_default();
        let commitment_level = config
            .account_config
            .commitment
            .unwrap_or_else(CommitmentConfig::recent)
            .commitment;
        let mut subscriptions = if commitment_level == CommitmentLevel::SingleGossip {
            self.subscriptions
                .gossip_program_subscriptions
                .write()
                .unwrap()
        } else {
            self.subscriptions.program_subscriptions.write().unwrap()
        };
        add_subscription(
            &mut subscriptions,
            program_id,
            config.account_config.commitment,
            sub_id,
            subscriber,
            0, // last_notified_slot is not utilized for program subscriptions
            Some(ProgramConfig {
                filters: config.filters.unwrap_or_default(),
                encoding: config.account_config.encoding,
            }),
        );
    }

    pub fn remove_program_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.subscriptions.program_subscriptions.write().unwrap();
        if remove_subscription(&mut subscriptions, id) {
            true
        } else {
            let mut subscriptions = self
                .subscriptions
                .gossip_program_subscriptions
                .write()
                .unwrap();
            remove_subscription(&mut subscriptions, id)
        }
    }

    pub fn add_signature_subscription(
        &self,
        signature: Signature,
        signature_subscribe_config: Option<RpcSignatureSubscribeConfig>,
        sub_id: SubscriptionId,
        subscriber: Subscriber<Response<RpcSignatureResult>>,
    ) {
        let (commitment, enable_received_notification) = signature_subscribe_config
            .map(|config| (config.commitment, config.enable_received_notification))
            .unwrap_or_default();

        let commitment_level = commitment
            .unwrap_or_else(CommitmentConfig::recent)
            .commitment;

        let mut subscriptions = if commitment_level == CommitmentLevel::SingleGossip {
            self.subscriptions
                .gossip_signature_subscriptions
                .write()
                .unwrap()
        } else {
            self.subscriptions.signature_subscriptions.write().unwrap()
        };
        add_subscription(
            &mut subscriptions,
            signature,
            commitment,
            sub_id,
            subscriber,
            0, // last_notified_slot is not utilized for signature subscriptions
            enable_received_notification,
        );
    }

    pub fn remove_signature_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.subscriptions.signature_subscriptions.write().unwrap();
        if remove_subscription(&mut subscriptions, id) {
            true
        } else {
            let mut subscriptions = self
                .subscriptions
                .gossip_signature_subscriptions
                .write()
                .unwrap();
            remove_subscription(&mut subscriptions, id)
        }
    }

    /// Notify subscribers of changes to any accounts or new signatures since
    /// the bank's last checkpoint.
    pub fn notify_subscribers(&self, commitment_slots: CommitmentSlots) {
        self.enqueue_notification(NotificationEntry::Bank(commitment_slots));
    }

    /// Notify SingleGossip commitment-level subscribers of changes to any accounts or new
    /// signatures.
    pub fn notify_gossip_subscribers(&self, slot: Slot) {
        self.enqueue_notification(NotificationEntry::Gossip(slot));
    }

    pub fn add_slot_subscription(&self, sub_id: SubscriptionId, subscriber: Subscriber<SlotInfo>) {
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let mut subscriptions = self.subscriptions.slot_subscriptions.write().unwrap();
        subscriptions.insert(sub_id, sink);
    }

    pub fn remove_slot_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.subscriptions.slot_subscriptions.write().unwrap();
        subscriptions.remove(id).is_some()
    }

    pub fn notify_slot(&self, slot: Slot, parent: Slot, root: Slot) {
        self.enqueue_notification(NotificationEntry::Slot(SlotInfo { slot, parent, root }));
    }

    pub fn notify_signatures_received(&self, slot_signatures: (Slot, Vec<Signature>)) {
        self.enqueue_notification(NotificationEntry::SignaturesReceived(slot_signatures));
    }

    pub fn add_vote_subscription(&self, sub_id: SubscriptionId, subscriber: Subscriber<RpcVote>) {
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let mut subscriptions = self.subscriptions.vote_subscriptions.write().unwrap();
        subscriptions.insert(sub_id, sink);
    }

    pub fn remove_vote_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.subscriptions.vote_subscriptions.write().unwrap();
        subscriptions.remove(id).is_some()
    }

    pub fn notify_vote(&self, vote: &Vote) {
        self.enqueue_notification(NotificationEntry::Vote(vote.clone()));
    }

    pub fn notify_frozen(&self, frozen_slot: Slot) {
        self.enqueue_notification(NotificationEntry::Frozen(frozen_slot));
    }

    pub fn add_root_subscription(&self, sub_id: SubscriptionId, subscriber: Subscriber<Slot>) {
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let mut subscriptions = self.subscriptions.root_subscriptions.write().unwrap();
        subscriptions.insert(sub_id, sink);
    }

    pub fn remove_root_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.subscriptions.root_subscriptions.write().unwrap();
        subscriptions.remove(id).is_some()
    }

    pub fn notify_roots(&self, mut rooted_slots: Vec<Slot>) {
        rooted_slots.sort();
        rooted_slots.into_iter().for_each(|root| {
            self.enqueue_notification(NotificationEntry::Root(root));
        });
    }

    fn enqueue_notification(&self, notification_entry: NotificationEntry) {
        match self
            .notification_sender
            .lock()
            .unwrap()
            .send(notification_entry)
        {
            Ok(()) => (),
            Err(SendError(notification)) => {
                warn!(
                    "Dropped RPC Notification - receiver disconnected : {:?}",
                    notification
                );
            }
        }
    }

    fn process_notifications(
        exit: Arc<AtomicBool>,
        notifier: RpcNotifier,
        notification_receiver: Receiver<NotificationEntry>,
        subscriptions: Subscriptions,
        bank_forks: Arc<RwLock<BankForks>>,
        last_checked_slots: Arc<RwLock<HashMap<CommitmentLevel, Slot>>>,
    ) {
        let mut pending_gossip_notifications = HashSet::new();
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            match notification_receiver.recv_timeout(Duration::from_millis(RECEIVE_DELAY_MILLIS)) {
                Ok(notification_entry) => match notification_entry {
                    NotificationEntry::Slot(slot_info) => {
                        let subscriptions = subscriptions.slot_subscriptions.read().unwrap();
                        for (_, sink) in subscriptions.iter() {
                            notifier.notify(slot_info, sink);
                        }
                    }
                    // These notifications are only triggered by votes observed on gossip,
                    // unlike `NotificationEntry::Gossip`, which also accounts for slots seen
                    // in VoteState's from bank states built in ReplayStage.
                    NotificationEntry::Vote(ref vote_info) => {
                        let subscriptions = subscriptions.vote_subscriptions.read().unwrap();
                        for (_, sink) in subscriptions.iter() {
                            notifier.notify(
                                RpcVote {
                                    slots: vote_info.slots.clone(),
                                    hash: bs58::encode(vote_info.hash).into_string(),
                                    timestamp: vote_info.timestamp,
                                },
                                sink,
                            );
                        }
                    }
                    NotificationEntry::Root(root) => {
                        let subscriptions = subscriptions.root_subscriptions.read().unwrap();
                        for (_, sink) in subscriptions.iter() {
                            notifier.notify(root, sink);
                        }

                        // Prune old pending notifications
                        pending_gossip_notifications = pending_gossip_notifications
                            .into_iter()
                            .filter(|&s| s > root)
                            .collect();
                    }
                    NotificationEntry::Bank(commitment_slots) => {
                        RpcSubscriptions::notify_accounts_programs_signatures(
                            &subscriptions.account_subscriptions,
                            &subscriptions.program_subscriptions,
                            &subscriptions.signature_subscriptions,
                            &bank_forks,
                            &commitment_slots,
                            &notifier,
                        )
                    }
                    NotificationEntry::Frozen(slot) => {
                        if pending_gossip_notifications.remove(&slot) {
                            Self::process_gossip_notification(
                                slot,
                                &notifier,
                                &subscriptions,
                                &bank_forks,
                                &last_checked_slots,
                            );
                        }
                    }
                    NotificationEntry::Gossip(slot) => {
                        let bank_frozen = bank_forks
                            .read()
                            .unwrap()
                            .get(slot)
                            .filter(|b| b.is_frozen())
                            .is_some();

                        if !bank_frozen {
                            pending_gossip_notifications.insert(slot);
                        } else {
                            Self::process_gossip_notification(
                                slot,
                                &notifier,
                                &subscriptions,
                                &bank_forks,
                                &last_checked_slots,
                            );
                        }
                    }
                    NotificationEntry::SignaturesReceived(slot_signatures) => {
                        RpcSubscriptions::process_signatures_received(
                            &slot_signatures,
                            &subscriptions.signature_subscriptions,
                            &notifier,
                        )
                    }
                },
                Err(RecvTimeoutError::Timeout) => {
                    // not a problem - try reading again
                }
                Err(RecvTimeoutError::Disconnected) => {
                    warn!("RPC Notification thread - sender disconnected");
                    break;
                }
            }
        }
    }

    fn process_gossip_notification(
        slot: Slot,
        notifier: &RpcNotifier,
        subscriptions: &Subscriptions,
        bank_forks: &Arc<RwLock<BankForks>>,
        last_checked_slots: &Arc<RwLock<HashMap<CommitmentLevel, Slot>>>,
    ) {
        let mut last_checked_slots_lock = last_checked_slots.write().unwrap();
        let last_checked_slot = last_checked_slots_lock
            .get(&CommitmentLevel::SingleGossip)
            .cloned()
            .unwrap_or_default();

        if slot > last_checked_slot {
            last_checked_slots_lock.insert(CommitmentLevel::SingleGossip, slot);
        } else {
            // Avoid sending stale or duplicate notifications
            return;
        }

        drop(last_checked_slots_lock);

        let commitment_slots = CommitmentSlots {
            highest_confirmed_slot: slot,
            ..CommitmentSlots::default()
        };
        RpcSubscriptions::notify_accounts_programs_signatures(
            &subscriptions.gossip_account_subscriptions,
            &subscriptions.gossip_program_subscriptions,
            &subscriptions.gossip_signature_subscriptions,
            bank_forks,
            &commitment_slots,
            &notifier,
        );
    }

    fn notify_accounts_programs_signatures(
        account_subscriptions: &Arc<RpcAccountSubscriptions>,
        program_subscriptions: &Arc<RpcProgramSubscriptions>,
        signature_subscriptions: &Arc<RpcSignatureSubscriptions>,
        bank_forks: &Arc<RwLock<BankForks>>,
        commitment_slots: &CommitmentSlots,
        notifier: &RpcNotifier,
    ) {
        let pubkeys: Vec<_> = {
            let subs = account_subscriptions.read().unwrap();
            subs.keys().cloned().collect()
        };
        for pubkey in &pubkeys {
            Self::check_account(
                pubkey,
                bank_forks,
                account_subscriptions.clone(),
                &notifier,
                &commitment_slots,
            );
        }

        let programs: Vec<_> = {
            let subs = program_subscriptions.read().unwrap();
            subs.keys().cloned().collect()
        };
        for program_id in &programs {
            Self::check_program(
                program_id,
                bank_forks,
                program_subscriptions.clone(),
                &notifier,
                &commitment_slots,
            );
        }

        let signatures: Vec<_> = {
            let subs = signature_subscriptions.read().unwrap();
            subs.keys().cloned().collect()
        };
        for signature in &signatures {
            Self::check_signature(
                signature,
                bank_forks,
                signature_subscriptions.clone(),
                &notifier,
                &commitment_slots,
            );
        }
    }

    fn process_signatures_received(
        (received_slot, signatures): &(Slot, Vec<Signature>),
        signature_subscriptions: &Arc<RpcSignatureSubscriptions>,
        notifier: &RpcNotifier,
    ) {
        for signature in signatures {
            if let Some(hashmap) = signature_subscriptions.read().unwrap().get(signature) {
                for (
                    _,
                    SubscriptionData {
                        sink,
                        config: is_received_notification_enabled,
                        ..
                    },
                ) in hashmap.iter()
                {
                    if is_received_notification_enabled.unwrap_or_default() {
                        notifier.notify(
                            Response {
                                context: RpcResponseContext {
                                    slot: *received_slot,
                                },
                                value: RpcSignatureResult::ReceivedSignature(
                                    ReceivedSignatureResult::ReceivedSignature,
                                ),
                            },
                            &sink,
                        );
                    }
                }
            }
        }
    }

    fn shutdown(&mut self) -> std::thread::Result<()> {
        if let Some(runtime) = self.notifier_runtime.take() {
            info!("RPC Notifier runtime - shutting down");
            let _ = runtime.shutdown_now().wait();
            info!("RPC Notifier runtime - shut down");
        }

        if self.t_cleanup.is_some() {
            info!("RPC Notification thread - shutting down");
            self.exit.store(true, Ordering::Relaxed);
            let x = self.t_cleanup.take().unwrap().join();
            info!("RPC Notification thread - shut down.");
            x
        } else {
            warn!("RPC Notification thread - already shut down.");
            Ok(())
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use jsonrpc_core::futures::{self, stream::Stream};
    use jsonrpc_pubsub::typed::Subscriber;
    use serial_test_derive::serial;
    use solana_runtime::{
        commitment::BlockCommitment,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
    };
    use solana_sdk::{
        signature::{Keypair, Signer},
        system_transaction,
    };
    use std::{fmt::Debug, sync::mpsc::channel, time::Instant};
    use tokio_01::{prelude::FutureExt, runtime::Runtime, timer::Delay};

    pub(crate) fn robust_poll_or_panic<T: Debug + Send + 'static>(
        receiver: futures::sync::mpsc::Receiver<T>,
    ) -> (T, futures::sync::mpsc::Receiver<T>) {
        let (inner_sender, inner_receiver) = channel();
        let mut rt = Runtime::new().unwrap();
        rt.spawn(futures::lazy(|| {
            let recv_timeout = receiver
                .into_future()
                .timeout(Duration::from_millis(RECEIVE_DELAY_MILLIS))
                .map(move |result| match result {
                    (Some(value), receiver) => {
                        inner_sender.send((value, receiver)).expect("send error")
                    }
                    (None, _) => panic!("unexpected end of stream"),
                })
                .map_err(|err| panic!("stream error {:?}", err));

            const INITIAL_DELAY_MS: u64 = RECEIVE_DELAY_MILLIS * 2;
            Delay::new(Instant::now() + Duration::from_millis(INITIAL_DELAY_MS))
                .and_then(|_| recv_timeout)
                .map_err(|err| panic!("timer error {:?}", err))
        }));
        inner_receiver.recv().expect("recv error")
    }

    #[test]
    #[serial]
    fn test_check_account_subscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let bank0 = bank_forks.read().unwrap().get(0).unwrap().clone();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);
        let alice = Keypair::new();

        let (subscriber, _id_receiver, transport_receiver) =
            Subscriber::new_test("accountNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let exit = Arc::new(AtomicBool::new(false));
        let subscriptions = RpcSubscriptions::new(
            &exit,
            bank_forks.clone(),
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests_with_slots(
                1, 1,
            ))),
        );
        subscriptions.add_account_subscription(
            alice.pubkey(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::recent()),
                encoding: None,
                data_slice: None,
            }),
            sub_id.clone(),
            subscriber,
        );

        assert!(subscriptions
            .subscriptions
            .account_subscriptions
            .read()
            .unwrap()
            .contains_key(&alice.pubkey()));

        let tx = system_transaction::create_account(
            &mint_keypair,
            &alice,
            blockhash,
            1,
            16,
            &solana_stake_program::id(),
        );
        bank_forks
            .write()
            .unwrap()
            .get(1)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();
        let mut commitment_slots = CommitmentSlots::default();
        commitment_slots.slot = 1;
        subscriptions.notify_subscribers(commitment_slots);
        let (response, _) = robust_poll_or_panic(transport_receiver);
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "context": { "slot": 1 },
                   "value": {
                       "data": "1111111111111111",
                       "executable": false,
                       "lamports": 1,
                       "owner": "Stake11111111111111111111111111111111111111",
                       "rentEpoch": 0,
                    },
               },
               "subscription": 0,
           }
        });
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);

        subscriptions.remove_account_subscription(&sub_id);
        assert!(!subscriptions
            .subscriptions
            .account_subscriptions
            .read()
            .unwrap()
            .contains_key(&alice.pubkey()));
    }

    #[test]
    #[serial]
    fn test_check_program_subscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let alice = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &alice,
            blockhash,
            1,
            16,
            &solana_stake_program::id(),
        );
        bank_forks
            .write()
            .unwrap()
            .get(0)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();

        let (subscriber, _id_receiver, transport_receiver) =
            Subscriber::new_test("programNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let exit = Arc::new(AtomicBool::new(false));
        let subscriptions = RpcSubscriptions::new(
            &exit,
            bank_forks,
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
        );
        subscriptions.add_program_subscription(
            solana_stake_program::id(),
            None,
            sub_id.clone(),
            subscriber,
        );

        assert!(subscriptions
            .subscriptions
            .program_subscriptions
            .read()
            .unwrap()
            .contains_key(&solana_stake_program::id()));

        subscriptions.notify_subscribers(CommitmentSlots::default());
        let (response, _) = robust_poll_or_panic(transport_receiver);
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "programNotification",
           "params": {
               "result": {
                   "context": { "slot": 0 },
                   "value": {
                       "account": {
                          "data": "1111111111111111",
                          "executable": false,
                          "lamports": 1,
                          "owner": "Stake11111111111111111111111111111111111111",
                          "rentEpoch": 0,
                       },
                       "pubkey": alice.pubkey().to_string(),
                    },
               },
               "subscription": 0,
           }
        });
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);

        subscriptions.remove_program_subscription(&sub_id);
        assert!(!subscriptions
            .subscriptions
            .program_subscriptions
            .read()
            .unwrap()
            .contains_key(&solana_stake_program::id()));
    }

    #[test]
    #[serial]
    fn test_check_signature_subscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let mut bank_forks = BankForks::new(bank);
        let alice = Keypair::new();

        let past_bank_tx =
            system_transaction::transfer(&mint_keypair, &alice.pubkey(), 1, blockhash);
        let unprocessed_tx =
            system_transaction::transfer(&mint_keypair, &alice.pubkey(), 2, blockhash);
        let processed_tx =
            system_transaction::transfer(&mint_keypair, &alice.pubkey(), 3, blockhash);

        bank_forks
            .get(0)
            .unwrap()
            .process_transaction(&past_bank_tx)
            .unwrap();

        let next_bank =
            Bank::new_from_parent(&bank_forks.banks[&0].clone(), &Pubkey::new_rand(), 1);
        bank_forks.insert(next_bank);

        bank_forks
            .get(1)
            .unwrap()
            .process_transaction(&processed_tx)
            .unwrap();
        let bank1 = bank_forks[1].clone();

        let bank_forks = Arc::new(RwLock::new(bank_forks));

        let mut cache0 = BlockCommitment::default();
        cache0.increase_confirmation_stake(1, 10);
        let cache1 = BlockCommitment::default();

        let mut block_commitment = HashMap::new();
        block_commitment.entry(0).or_insert(cache0);
        block_commitment.entry(1).or_insert(cache1);
        let block_commitment_cache = BlockCommitmentCache::new(
            block_commitment,
            10,
            CommitmentSlots {
                slot: bank1.slot(),
                ..CommitmentSlots::default()
            },
        );

        let exit = Arc::new(AtomicBool::new(false));
        let subscriptions = RpcSubscriptions::new(
            &exit,
            bank_forks,
            Arc::new(RwLock::new(block_commitment_cache)),
        );

        let (past_bank_sub1, _id_receiver, past_bank_recv1) =
            Subscriber::new_test("signatureNotification");
        let (past_bank_sub2, _id_receiver, past_bank_recv2) =
            Subscriber::new_test("signatureNotification");
        let (processed_sub, _id_receiver, processed_recv) =
            Subscriber::new_test("signatureNotification");
        let (processed_sub3, _id_receiver, processed_recv3) =
            Subscriber::new_test("signatureNotification");

        subscriptions.add_signature_subscription(
            past_bank_tx.signatures[0],
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::recent()),
                enable_received_notification: Some(false),
            }),
            SubscriptionId::Number(1 as u64),
            past_bank_sub1,
        );
        subscriptions.add_signature_subscription(
            past_bank_tx.signatures[0],
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::root()),
                enable_received_notification: Some(false),
            }),
            SubscriptionId::Number(2 as u64),
            past_bank_sub2,
        );
        subscriptions.add_signature_subscription(
            processed_tx.signatures[0],
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::recent()),
                enable_received_notification: Some(false),
            }),
            SubscriptionId::Number(3 as u64),
            processed_sub,
        );
        subscriptions.add_signature_subscription(
            unprocessed_tx.signatures[0],
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::recent()),
                enable_received_notification: Some(false),
            }),
            SubscriptionId::Number(4 as u64),
            Subscriber::new_test("signatureNotification").0,
        );
        // Add a subscription that gets `received` notifications
        subscriptions.add_signature_subscription(
            unprocessed_tx.signatures[0],
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::recent()),
                enable_received_notification: Some(true),
            }),
            SubscriptionId::Number(5 as u64),
            processed_sub3,
        );

        {
            let sig_subs = subscriptions
                .subscriptions
                .signature_subscriptions
                .read()
                .unwrap();
            assert_eq!(sig_subs.get(&past_bank_tx.signatures[0]).unwrap().len(), 2);
            assert!(sig_subs.contains_key(&unprocessed_tx.signatures[0]));
            assert!(sig_subs.contains_key(&processed_tx.signatures[0]));
        }
        let mut commitment_slots = CommitmentSlots::default();
        let received_slot = 1;
        commitment_slots.slot = received_slot;
        subscriptions
            .notify_signatures_received((received_slot, vec![unprocessed_tx.signatures[0]]));
        subscriptions.notify_subscribers(commitment_slots);
        let expected_res =
            RpcSignatureResult::ProcessedSignature(ProcessedSignatureResult { err: None });
        let received_expected_res =
            RpcSignatureResult::ReceivedSignature(ReceivedSignatureResult::ReceivedSignature);
        struct Notification {
            slot: Slot,
            id: u64,
        }

        let expected_notification =
            |exp: Notification, expected_res: &RpcSignatureResult| -> String {
                let json = json!({
                    "jsonrpc": "2.0",
                    "method": "signatureNotification",
                    "params": {
                        "result": {
                            "context": { "slot": exp.slot },
                            "value": expected_res,
                        },
                        "subscription": exp.id,
                    }
                });
                serde_json::to_string(&json).unwrap()
            };

        // Expect to receive a notification from bank 1 because this subscription is
        // looking for 0 confirmations and so checks the current bank
        let expected = expected_notification(Notification { slot: 1, id: 1 }, &expected_res);
        let (response, _) = robust_poll_or_panic(past_bank_recv1);
        assert_eq!(expected, response);

        // Expect to receive a notification from bank 0 because this subscription is
        // looking for 1 confirmation and so checks the past bank
        let expected = expected_notification(Notification { slot: 0, id: 2 }, &expected_res);
        let (response, _) = robust_poll_or_panic(past_bank_recv2);
        assert_eq!(expected, response);

        let expected = expected_notification(Notification { slot: 1, id: 3 }, &expected_res);
        let (response, _) = robust_poll_or_panic(processed_recv);
        assert_eq!(expected, response);

        // Expect a "received" notification
        let expected = expected_notification(
            Notification {
                slot: received_slot,
                id: 5,
            },
            &received_expected_res,
        );
        let (response, _) = robust_poll_or_panic(processed_recv3);
        assert_eq!(expected, response);

        // Subscription should be automatically removed after notification
        let sig_subs = subscriptions
            .subscriptions
            .signature_subscriptions
            .read()
            .unwrap();
        assert!(!sig_subs.contains_key(&processed_tx.signatures[0]));
        assert!(!sig_subs.contains_key(&past_bank_tx.signatures[0]));

        // Unprocessed signature subscription should not be removed
        assert_eq!(
            sig_subs.get(&unprocessed_tx.signatures[0]).unwrap().len(),
            2
        );
    }

    #[test]
    #[serial]
    fn test_check_slot_subscribe() {
        let (subscriber, _id_receiver, transport_receiver) =
            Subscriber::new_test("slotNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let exit = Arc::new(AtomicBool::new(false));
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let subscriptions = RpcSubscriptions::new(
            &exit,
            bank_forks,
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
        );
        subscriptions.add_slot_subscription(sub_id.clone(), subscriber);

        assert!(subscriptions
            .subscriptions
            .slot_subscriptions
            .read()
            .unwrap()
            .contains_key(&sub_id));

        subscriptions.notify_slot(0, 0, 0);
        let (response, _) = robust_poll_or_panic(transport_receiver);
        let expected_res = SlotInfo {
            parent: 0,
            slot: 0,
            root: 0,
        };
        let expected_res_str =
            serde_json::to_string(&serde_json::to_value(expected_res).unwrap()).unwrap();
        let expected = format!(
            r#"{{"jsonrpc":"2.0","method":"slotNotification","params":{{"result":{},"subscription":0}}}}"#,
            expected_res_str
        );
        assert_eq!(expected, response);

        subscriptions.remove_slot_subscription(&sub_id);
        assert!(!subscriptions
            .subscriptions
            .slot_subscriptions
            .read()
            .unwrap()
            .contains_key(&sub_id));
    }

    #[test]
    #[serial]
    fn test_check_root_subscribe() {
        let (subscriber, _id_receiver, mut transport_receiver) =
            Subscriber::new_test("rootNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let exit = Arc::new(AtomicBool::new(false));
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let subscriptions = RpcSubscriptions::new(
            &exit,
            bank_forks,
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
        );
        subscriptions.add_root_subscription(sub_id.clone(), subscriber);

        assert!(subscriptions
            .subscriptions
            .root_subscriptions
            .read()
            .unwrap()
            .contains_key(&sub_id));

        subscriptions.notify_roots(vec![2, 1, 3]);

        for expected_root in 1..=3 {
            let (response, receiver) = robust_poll_or_panic(transport_receiver);
            transport_receiver = receiver;
            let expected_res_str =
                serde_json::to_string(&serde_json::to_value(expected_root).unwrap()).unwrap();
            let expected = format!(
                r#"{{"jsonrpc":"2.0","method":"rootNotification","params":{{"result":{},"subscription":0}}}}"#,
                expected_res_str
            );
            assert_eq!(expected, response);
        }

        subscriptions.remove_root_subscription(&sub_id);
        assert!(!subscriptions
            .subscriptions
            .root_subscriptions
            .read()
            .unwrap()
            .contains_key(&sub_id));
    }

    #[test]
    #[serial]
    fn test_add_and_remove_subscription() {
        let mut subscriptions: HashMap<u64, HashMap<SubscriptionId, SubscriptionData<(), ()>>> =
            HashMap::new();

        let num_keys = 5;
        for key in 0..num_keys {
            let (subscriber, _id_receiver, _transport_receiver) =
                Subscriber::new_test("notification");
            let sub_id = SubscriptionId::Number(key);
            add_subscription(&mut subscriptions, key, None, sub_id, subscriber, 0, None);
        }

        // Add another subscription to the "0" key
        let (subscriber, _id_receiver, _transport_receiver) = Subscriber::new_test("notification");
        let extra_sub_id = SubscriptionId::Number(num_keys);
        add_subscription(
            &mut subscriptions,
            0,
            None,
            extra_sub_id.clone(),
            subscriber,
            0,
            None,
        );

        assert_eq!(subscriptions.len(), num_keys as usize);
        assert_eq!(subscriptions.get(&0).unwrap().len(), 2);
        assert_eq!(subscriptions.get(&1).unwrap().len(), 1);

        assert_eq!(
            remove_subscription(&mut subscriptions, &SubscriptionId::Number(0)),
            true
        );
        assert_eq!(subscriptions.len(), num_keys as usize);
        assert_eq!(subscriptions.get(&0).unwrap().len(), 1);
        assert_eq!(
            remove_subscription(&mut subscriptions, &SubscriptionId::Number(0)),
            false
        );

        assert_eq!(remove_subscription(&mut subscriptions, &extra_sub_id), true);
        assert_eq!(subscriptions.len(), (num_keys - 1) as usize);
        assert!(subscriptions.get(&0).is_none());
    }

    #[test]
    #[serial]
    fn test_gossip_separate_account_notifications() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let bank0 = bank_forks.read().unwrap().get(0).unwrap().clone();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);
        let bank2 = Bank::new_from_parent(&bank0, &Pubkey::default(), 2);
        bank_forks.write().unwrap().insert(bank2);
        let alice = Keypair::new();

        let (subscriber0, _id_receiver, transport_receiver0) =
            Subscriber::new_test("accountNotification");
        let (subscriber1, _id_receiver, transport_receiver1) =
            Subscriber::new_test("accountNotification");
        let exit = Arc::new(AtomicBool::new(false));
        let subscriptions = RpcSubscriptions::new(
            &exit,
            bank_forks.clone(),
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests_with_slots(
                1, 1,
            ))),
        );
        let sub_id0 = SubscriptionId::Number(0 as u64);
        subscriptions.add_account_subscription(
            alice.pubkey(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::single_gossip()),
                encoding: None,
                data_slice: None,
            }),
            sub_id0.clone(),
            subscriber0,
        );

        assert!(subscriptions
            .subscriptions
            .gossip_account_subscriptions
            .read()
            .unwrap()
            .contains_key(&alice.pubkey()));

        let tx = system_transaction::create_account(
            &mint_keypair,
            &alice,
            blockhash,
            1,
            16,
            &solana_stake_program::id(),
        );

        // Add the transaction to the 1st bank and then freeze the bank
        let bank1 = bank_forks.write().unwrap().get(1).cloned().unwrap();
        bank1.process_transaction(&tx).unwrap();
        bank1.freeze();

        // Add the same transaction to the unfrozen 2nd bank
        bank_forks
            .write()
            .unwrap()
            .get(2)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();

        // First, notify the unfrozen bank first to queue pending notification
        subscriptions.notify_gossip_subscribers(2);

        // Now, notify the frozen bank and ensure its notifications are processed
        subscriptions.notify_gossip_subscribers(1);

        let (response, _) = robust_poll_or_panic(transport_receiver0);
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "context": { "slot": 1 },
                   "value": {
                       "data": "1111111111111111",
                       "executable": false,
                       "lamports": 1,
                       "owner": "Stake11111111111111111111111111111111111111",
                       "rentEpoch": 0,
                    },
               },
               "subscription": 0,
           }
        });
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);
        subscriptions.remove_account_subscription(&sub_id0);

        let sub_id1 = SubscriptionId::Number(1 as u64);
        subscriptions.add_account_subscription(
            alice.pubkey(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::single_gossip()),
                encoding: None,
                data_slice: None,
            }),
            sub_id1.clone(),
            subscriber1,
        );

        subscriptions.notify_frozen(2);
        let (response, _) = robust_poll_or_panic(transport_receiver1);
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "context": { "slot": 2 },
                   "value": {
                       "data": "1111111111111111",
                       "executable": false,
                       "lamports": 1,
                       "owner": "Stake11111111111111111111111111111111111111",
                       "rentEpoch": 0,
                    },
               },
               "subscription": 1,
           }
        });
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);
        subscriptions.remove_account_subscription(&sub_id1);

        assert!(!subscriptions
            .subscriptions
            .gossip_account_subscriptions
            .read()
            .unwrap()
            .contains_key(&alice.pubkey()));
    }
}

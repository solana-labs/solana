//! The `pubsub` module implements a threaded subscription service on client RPC request

use {
    crate::{
        optimistically_confirmed_bank_tracker::OptimisticallyConfirmedBank,
        parsed_token_accounts::{get_parsed_token_account, get_parsed_token_accounts},
    },
    core::hash::Hash,
    jsonrpc_pubsub::{
        typed::{Sink, Subscriber},
        SubscriptionId,
    },
    serde::Serialize,
    solana_account_decoder::{parse_token::spl_token_id_v2_0, UiAccount, UiAccountEncoding},
    solana_client::{
        rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcSignatureSubscribeConfig},
        rpc_filter::RpcFilterType,
        rpc_response::{
            ProcessedSignatureResult, ReceivedSignatureResult, Response, RpcKeyedAccount,
            RpcLogsResponse, RpcResponseContext, RpcSignatureResult, SlotInfo, SlotUpdate,
        },
    },
    solana_measure::measure::Measure,
    solana_runtime::{
        bank::{
            Bank, TransactionLogCollectorConfig, TransactionLogCollectorFilter, TransactionLogInfo,
        },
        bank_forks::BankForks,
        commitment::{BlockCommitmentCache, CommitmentSlots},
    },
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::{Slot, UnixTimestamp},
        commitment_config::CommitmentConfig,
        pubkey::Pubkey,
        signature::Signature,
        timing::timestamp,
        transaction,
    },
    solana_vote_program::vote_state::Vote,
    std::{
        collections::{HashMap, HashSet},
        iter,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc::{Receiver, RecvTimeoutError, SendError, Sender},
        },
        sync::{Arc, Mutex, RwLock},
        thread::{Builder, JoinHandle},
        time::Duration,
    },
};

const RECEIVE_DELAY_MILLIS: u64 = 100;

trait BankGetTransactionLogsAdapter {
    fn get_transaction_logs_adapter(
        &self,
        stuff: &(Option<Pubkey>, bool),
    ) -> Option<Vec<TransactionLogInfo>>;
}

impl BankGetTransactionLogsAdapter for Bank {
    fn get_transaction_logs_adapter(
        &self,
        config: &(Option<Pubkey>, bool),
    ) -> Option<Vec<TransactionLogInfo>> {
        let mut logs = self.get_transaction_logs(config.0.as_ref());

        if config.0.is_none() && !config.1 {
            // Filter out votes if the subscriber doesn't want them
            logs = logs.map(|logs| logs.into_iter().filter(|log| !log.is_vote).collect());
        }
        logs
    }
}

// A more human-friendly version of Vote, with the bank state signature base58 encoded.
#[derive(Serialize, Deserialize, Debug)]
pub struct RpcVote {
    pub slots: Vec<Slot>,
    pub hash: String,
    pub timestamp: Option<UnixTimestamp>,
}

enum NotificationEntry {
    Slot(SlotInfo),
    SlotUpdate(SlotUpdate),
    Vote(Vote),
    Root(Slot),
    Bank(CommitmentSlots),
    Gossip(Slot),
    SignaturesReceived((Slot, Vec<Signature>)),
}

impl std::fmt::Debug for NotificationEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            NotificationEntry::Root(root) => write!(f, "Root({})", root),
            NotificationEntry::Vote(vote) => write!(f, "Vote({:?})", vote),
            NotificationEntry::Slot(slot_info) => write!(f, "Slot({:?})", slot_info),
            NotificationEntry::SlotUpdate(slot_update) => {
                write!(f, "SlotUpdate({:?})", slot_update)
            }
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
type RpcLogsSubscriptions = RwLock<
    HashMap<
        (Option<Pubkey>, bool),
        HashMap<SubscriptionId, SubscriptionData<Response<RpcLogsResponse>, ()>>,
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
type RpcSlotUpdateSubscriptions = RwLock<HashMap<SubscriptionId, Sink<Arc<SlotUpdate>>>>;
type RpcVoteSubscriptions = RwLock<HashMap<SubscriptionId, Sink<RpcVote>>>;
type RpcRootSubscriptions = RwLock<HashMap<SubscriptionId, Sink<Slot>>>;

fn add_subscription<K, S, T>(
    subscriptions: &mut HashMap<K, HashMap<SubscriptionId, SubscriptionData<S, T>>>,
    hashmap_key: K,
    commitment: CommitmentConfig,
    sub_id: SubscriptionId,
    subscriber: Subscriber<S>,
    last_notified_slot: Slot,
    config: Option<T>,
) where
    K: Eq + Hash,
    S: Clone,
{
    let sink = subscriber.assign_id(sub_id.clone()).unwrap();
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
    F: Fn(X, &K, Slot, Option<T>, Arc<Bank>) -> (Box<dyn Iterator<Item = S>>, Slot),
    X: Clone + Default,
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
            let slot = if commitment.is_finalized() {
                commitment_slots.highest_confirmed_root
            } else if commitment.is_confirmed() {
                commitment_slots.highest_confirmed_slot
            } else {
                commitment_slots.slot
            };

            if let Some(bank) = bank_forks.read().unwrap().get(slot).cloned() {
                let results = bank_method(&bank, hashmap_key);
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
    }
    notified_set
}

struct RpcNotifier;

impl RpcNotifier {
    fn notify<T>(&self, value: T, sink: &Sink<T>)
    where
        T: serde::Serialize,
    {
        let _ = sink.notify(Ok(value));
    }
}

fn filter_account_result(
    result: Option<(AccountSharedData, Slot)>,
    pubkey: &Pubkey,
    last_notified_slot: Slot,
    encoding: Option<UiAccountEncoding>,
    bank: Arc<Bank>,
) -> (Box<dyn Iterator<Item = UiAccount>>, Slot) {
    // If the account is not found, `last_modified_slot` will default to zero and
    // we will notify clients that the account no longer exists if we haven't already
    let (account, last_modified_slot) = result.unwrap_or_default();

    // If last_modified_slot < last_notified_slot this means that we last notified for a fork
    // and should notify that the account state has been reverted.
    let results: Box<dyn Iterator<Item = UiAccount>> = if last_modified_slot != last_notified_slot {
        let encoding = encoding.unwrap_or(UiAccountEncoding::Binary);
        if account.owner() == &spl_token_id_v2_0() && encoding == UiAccountEncoding::JsonParsed {
            Box::new(iter::once(get_parsed_token_account(bank, pubkey, account)))
        } else {
            Box::new(iter::once(UiAccount::encode(
                pubkey, &account, encoding, None, None,
            )))
        }
    } else {
        Box::new(iter::empty())
    };

    (results, last_modified_slot)
}

fn filter_signature_result(
    result: Option<transaction::Result<()>>,
    _signature: &Signature,
    last_notified_slot: Slot,
    _config: Option<bool>,
    _bank: Arc<Bank>,
) -> (Box<dyn Iterator<Item = RpcSignatureResult>>, Slot) {
    (
        Box::new(result.into_iter().map(|result| {
            RpcSignatureResult::ProcessedSignature(ProcessedSignatureResult { err: result.err() })
        })),
        last_notified_slot,
    )
}

fn filter_program_results(
    accounts: Vec<(Pubkey, AccountSharedData)>,
    program_id: &Pubkey,
    last_notified_slot: Slot,
    config: Option<ProgramConfig>,
    bank: Arc<Bank>,
) -> (Box<dyn Iterator<Item = RpcKeyedAccount>>, Slot) {
    let config = config.unwrap_or_default();
    let encoding = config.encoding.unwrap_or(UiAccountEncoding::Binary);
    let filters = config.filters;
    let accounts_is_empty = accounts.is_empty();
    let keyed_accounts = accounts.into_iter().filter(move |(_, account)| {
        filters.iter().all(|filter_type| match filter_type {
            RpcFilterType::DataSize(size) => account.data().len() as u64 == *size,
            RpcFilterType::Memcmp(compare) => compare.bytes_match(account.data()),
        })
    });
    let accounts: Box<dyn Iterator<Item = RpcKeyedAccount>> = if program_id == &spl_token_id_v2_0()
        && encoding == UiAccountEncoding::JsonParsed
        && !accounts_is_empty
    {
        Box::new(get_parsed_token_accounts(bank, keyed_accounts))
    } else {
        Box::new(
            keyed_accounts.map(move |(pubkey, account)| RpcKeyedAccount {
                pubkey: pubkey.to_string(),
                account: UiAccount::encode(&pubkey, &account, encoding, None, None),
            }),
        )
    };
    (accounts, last_notified_slot)
}

fn filter_logs_results(
    logs: Option<Vec<TransactionLogInfo>>,
    _address: &(Option<Pubkey>, bool),
    last_notified_slot: Slot,
    _config: Option<()>,
    _bank: Arc<Bank>,
) -> (Box<dyn Iterator<Item = RpcLogsResponse>>, Slot) {
    match logs {
        None => (Box::new(iter::empty()), last_notified_slot),
        Some(logs) => (
            Box::new(logs.into_iter().map(|log| RpcLogsResponse {
                signature: log.signature.to_string(),
                err: log.result.err(),
                logs: log.log_messages,
            })),
            last_notified_slot,
        ),
    }
}

fn total_nested_subscriptions<K, L, V>(
    subscription_map: &RwLock<HashMap<K, HashMap<L, V>>>,
) -> usize {
    subscription_map
        .read()
        .unwrap()
        .iter()
        .fold(0, |acc, x| acc + x.1.len())
}

#[derive(Clone)]
struct Subscriptions {
    account_subscriptions: Arc<RpcAccountSubscriptions>,
    program_subscriptions: Arc<RpcProgramSubscriptions>,
    logs_subscriptions: Arc<RpcLogsSubscriptions>,
    signature_subscriptions: Arc<RpcSignatureSubscriptions>,
    gossip_account_subscriptions: Arc<RpcAccountSubscriptions>,
    gossip_logs_subscriptions: Arc<RpcLogsSubscriptions>,
    gossip_program_subscriptions: Arc<RpcProgramSubscriptions>,
    gossip_signature_subscriptions: Arc<RpcSignatureSubscriptions>,
    slot_subscriptions: Arc<RpcSlotSubscriptions>,
    slots_updates_subscriptions: Arc<RpcSlotUpdateSubscriptions>,
    vote_subscriptions: Arc<RpcVoteSubscriptions>,
    root_subscriptions: Arc<RpcRootSubscriptions>,
}

impl Subscriptions {
    fn total(&self) -> usize {
        let mut total = 0;
        total += total_nested_subscriptions(&self.account_subscriptions);
        total += total_nested_subscriptions(&self.program_subscriptions);
        total += total_nested_subscriptions(&self.logs_subscriptions);
        total += total_nested_subscriptions(&self.signature_subscriptions);
        total += total_nested_subscriptions(&self.gossip_account_subscriptions);
        total += total_nested_subscriptions(&self.gossip_logs_subscriptions);
        total += total_nested_subscriptions(&self.gossip_program_subscriptions);
        total += total_nested_subscriptions(&self.gossip_signature_subscriptions);
        total += self.slot_subscriptions.read().unwrap().len();
        total += self.vote_subscriptions.read().unwrap().len();
        total += self.root_subscriptions.read().unwrap().len();
        total
    }
}

pub struct RpcSubscriptions {
    subscriptions: Subscriptions,
    notification_sender: Arc<Mutex<Sender<NotificationEntry>>>,
    t_cleanup: Option<JoinHandle<()>>,
    bank_forks: Arc<RwLock<BankForks>>,
    block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    optimistically_confirmed_bank: Arc<RwLock<OptimisticallyConfirmedBank>>,
    exit: Arc<AtomicBool>,
    enable_vote_subscription: bool,
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
        optimistically_confirmed_bank: Arc<RwLock<OptimisticallyConfirmedBank>>,
    ) -> Self {
        Self::new_with_vote_subscription(
            exit,
            bank_forks,
            block_commitment_cache,
            optimistically_confirmed_bank,
            false,
        )
    }

    pub fn new_with_vote_subscription(
        exit: &Arc<AtomicBool>,
        bank_forks: Arc<RwLock<BankForks>>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        optimistically_confirmed_bank: Arc<RwLock<OptimisticallyConfirmedBank>>,
        enable_vote_subscription: bool,
    ) -> Self {
        let (notification_sender, notification_receiver): (
            Sender<NotificationEntry>,
            Receiver<NotificationEntry>,
        ) = std::sync::mpsc::channel();

        let account_subscriptions = Arc::new(RpcAccountSubscriptions::default());
        let logs_subscriptions = Arc::new(RpcLogsSubscriptions::default());
        let program_subscriptions = Arc::new(RpcProgramSubscriptions::default());
        let signature_subscriptions = Arc::new(RpcSignatureSubscriptions::default());
        let gossip_account_subscriptions = Arc::new(RpcAccountSubscriptions::default());
        let gossip_logs_subscriptions = Arc::new(RpcLogsSubscriptions::default());
        let gossip_program_subscriptions = Arc::new(RpcProgramSubscriptions::default());
        let gossip_signature_subscriptions = Arc::new(RpcSignatureSubscriptions::default());
        let slot_subscriptions = Arc::new(RpcSlotSubscriptions::default());
        let slots_updates_subscriptions = Arc::new(RpcSlotUpdateSubscriptions::default());
        let vote_subscriptions = Arc::new(RpcVoteSubscriptions::default());
        let root_subscriptions = Arc::new(RpcRootSubscriptions::default());
        let notification_sender = Arc::new(Mutex::new(notification_sender));

        let _bank_forks = bank_forks.clone();
        let _block_commitment_cache = block_commitment_cache.clone();
        let exit_clone = exit.clone();
        let subscriptions = Subscriptions {
            account_subscriptions,
            program_subscriptions,
            logs_subscriptions,
            signature_subscriptions,
            gossip_account_subscriptions,
            gossip_logs_subscriptions,
            gossip_program_subscriptions,
            gossip_signature_subscriptions,
            slot_subscriptions,
            slots_updates_subscriptions,
            vote_subscriptions,
            root_subscriptions,
        };
        let _subscriptions = subscriptions.clone();

        let notifier = RpcNotifier {};
        let t_cleanup = Builder::new()
            .name("solana-rpc-notifications".to_string())
            .spawn(move || {
                Self::process_notifications(
                    exit_clone,
                    notifier,
                    notification_receiver,
                    _subscriptions,
                    _bank_forks,
                );
            })
            .unwrap();

        Self {
            subscriptions,
            notification_sender,
            t_cleanup: Some(t_cleanup),
            bank_forks,
            block_commitment_cache,
            optimistically_confirmed_bank,
            exit: exit.clone(),
            enable_vote_subscription,
        }
    }

    // For tests only...
    pub fn default_with_bank_forks(bank_forks: Arc<RwLock<BankForks>>) -> Self {
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        Self::new_with_vote_subscription(
            &Arc::new(AtomicBool::new(false)),
            bank_forks,
            Arc::new(RwLock::new(BlockCommitmentCache::default())),
            optimistically_confirmed_bank,
            true,
        )
    }

    fn check_account(
        pubkey: &Pubkey,
        bank_forks: &Arc<RwLock<BankForks>>,
        account_subscriptions: Arc<RpcAccountSubscriptions>,
        notifier: &RpcNotifier,
        commitment_slots: &CommitmentSlots,
    ) -> HashSet<SubscriptionId> {
        let subscriptions = account_subscriptions.read().unwrap();
        check_commitment_and_notify(
            &subscriptions,
            pubkey,
            bank_forks,
            commitment_slots,
            Bank::get_account_modified_slot,
            filter_account_result,
            notifier,
        )
    }

    fn check_logs(
        address_with_enable_votes_flag: &(Option<Pubkey>, bool),
        bank_forks: &Arc<RwLock<BankForks>>,
        logs_subscriptions: Arc<RpcLogsSubscriptions>,
        notifier: &RpcNotifier,
        commitment_slots: &CommitmentSlots,
    ) -> HashSet<SubscriptionId> {
        let subscriptions = logs_subscriptions.read().unwrap();
        check_commitment_and_notify(
            &subscriptions,
            address_with_enable_votes_flag,
            bank_forks,
            commitment_slots,
            Bank::get_transaction_logs_adapter,
            filter_logs_results,
            notifier,
        )
    }

    fn check_program(
        program_id: &Pubkey,
        bank_forks: &Arc<RwLock<BankForks>>,
        program_subscriptions: Arc<RpcProgramSubscriptions>,
        notifier: &RpcNotifier,
        commitment_slots: &CommitmentSlots,
    ) -> HashSet<SubscriptionId> {
        let subscriptions = program_subscriptions.read().unwrap();
        check_commitment_and_notify(
            &subscriptions,
            program_id,
            bank_forks,
            commitment_slots,
            Bank::get_program_accounts_modified_since_parent,
            filter_program_results,
            notifier,
        )
    }

    fn check_signature(
        signature: &Signature,
        bank_forks: &Arc<RwLock<BankForks>>,
        signature_subscriptions: Arc<RpcSignatureSubscriptions>,
        notifier: &RpcNotifier,
        commitment_slots: &CommitmentSlots,
    ) -> HashSet<SubscriptionId> {
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
                subscriptions.remove(signature);
            }
        }
        notified_ids
    }

    pub fn total(&self) -> usize {
        self.subscriptions.total()
    }

    pub fn add_account_subscription(
        &self,
        pubkey: Pubkey,
        config: Option<RpcAccountInfoConfig>,
        sub_id: SubscriptionId,
        subscriber: Subscriber<Response<UiAccount>>,
    ) {
        let config = config.unwrap_or_default();
        let commitment = config.commitment.unwrap_or_default();

        let slot = if commitment.is_finalized() {
            self.block_commitment_cache
                .read()
                .unwrap()
                .highest_confirmed_root()
        } else if commitment.is_confirmed() {
            self.optimistically_confirmed_bank
                .read()
                .unwrap()
                .bank
                .slot()
        } else {
            self.block_commitment_cache.read().unwrap().slot()
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

        let mut subscriptions = if commitment.is_confirmed() {
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
            commitment,
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
        let commitment = config.account_config.commitment.unwrap_or_default();

        let mut subscriptions = if commitment.is_confirmed() {
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
            commitment,
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

    pub fn add_logs_subscription(
        &self,
        address: Option<Pubkey>,
        include_votes: bool,
        commitment: Option<CommitmentConfig>,
        sub_id: SubscriptionId,
        subscriber: Subscriber<Response<RpcLogsResponse>>,
    ) {
        let commitment = commitment.unwrap_or_default();

        {
            let mut subscriptions = if commitment.is_confirmed() {
                self.subscriptions
                    .gossip_logs_subscriptions
                    .write()
                    .unwrap()
            } else {
                self.subscriptions.logs_subscriptions.write().unwrap()
            };
            add_subscription(
                &mut subscriptions,
                (address, include_votes),
                commitment,
                sub_id,
                subscriber,
                0, // last_notified_slot is not utilized for logs subscriptions
                None,
            );
        }
        self.update_bank_transaction_log_keys();
    }

    pub fn remove_logs_subscription(&self, id: &SubscriptionId) -> bool {
        let mut removed = {
            let mut subscriptions = self.subscriptions.logs_subscriptions.write().unwrap();
            remove_subscription(&mut subscriptions, id)
        };

        if !removed {
            removed = {
                let mut subscriptions = self
                    .subscriptions
                    .gossip_logs_subscriptions
                    .write()
                    .unwrap();
                remove_subscription(&mut subscriptions, id)
            };
        }

        if removed {
            self.update_bank_transaction_log_keys();
        }
        removed
    }

    fn update_bank_transaction_log_keys(&self) {
        // Grab a write lock for both `logs_subscriptions` and `gossip_logs_subscriptions`, to
        // ensure `Bank::transaction_log_collector_config` is updated atomically.
        let logs_subscriptions = self.subscriptions.logs_subscriptions.write().unwrap();
        let gossip_logs_subscriptions = self
            .subscriptions
            .gossip_logs_subscriptions
            .write()
            .unwrap();

        let mut config = TransactionLogCollectorConfig::default();

        let mut all = false;
        let mut all_with_votes = false;
        let mut mentioned_address = false;
        for (address, with_votes) in logs_subscriptions
            .keys()
            .chain(gossip_logs_subscriptions.keys())
        {
            match address {
                None => {
                    if *with_votes {
                        all_with_votes = true;
                    } else {
                        all = true;
                    }
                }
                Some(address) => {
                    config.mentioned_addresses.insert(*address);
                    mentioned_address = true;
                }
            }
        }
        config.filter = if all_with_votes {
            TransactionLogCollectorFilter::AllWithVotes
        } else if all {
            TransactionLogCollectorFilter::All
        } else if mentioned_address {
            TransactionLogCollectorFilter::OnlyMentionedAddresses
        } else {
            TransactionLogCollectorFilter::None
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

        let commitment = commitment.unwrap_or_default();

        let mut subscriptions = if commitment.is_confirmed() {
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

    /// Notify Confirmed commitment-level subscribers of changes to any accounts or new
    /// signatures.
    pub fn notify_gossip_subscribers(&self, slot: Slot) {
        self.enqueue_notification(NotificationEntry::Gossip(slot));
    }

    pub fn notify_slot_update(&self, slot_update: SlotUpdate) {
        self.enqueue_notification(NotificationEntry::SlotUpdate(slot_update));
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

    pub fn add_slots_updates_subscription(
        &self,
        sub_id: SubscriptionId,
        subscriber: Subscriber<Arc<SlotUpdate>>,
    ) {
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let mut subscriptions = self
            .subscriptions
            .slots_updates_subscriptions
            .write()
            .unwrap();
        subscriptions.insert(sub_id, sink);
    }

    pub fn remove_slots_updates_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self
            .subscriptions
            .slots_updates_subscriptions
            .write()
            .unwrap();
        subscriptions.remove(id).is_some()
    }

    pub fn notify_slot(&self, slot: Slot, parent: Slot, root: Slot) {
        self.enqueue_notification(NotificationEntry::Slot(SlotInfo { slot, parent, root }));
        self.enqueue_notification(NotificationEntry::SlotUpdate(SlotUpdate::CreatedBank {
            slot,
            parent,
            timestamp: timestamp(),
        }));
    }

    pub fn notify_signatures_received(&self, slot_signatures: (Slot, Vec<Signature>)) {
        self.enqueue_notification(NotificationEntry::SignaturesReceived(slot_signatures));
    }

    pub fn add_vote_subscription(&self, sub_id: SubscriptionId, subscriber: Subscriber<RpcVote>) {
        if self.enable_vote_subscription {
            let sink = subscriber.assign_id(sub_id.clone()).unwrap();
            let mut subscriptions = self.subscriptions.vote_subscriptions.write().unwrap();
            subscriptions.insert(sub_id, sink);
        } else {
            let _ = subscriber.reject(jsonrpc_core::Error::new(
                jsonrpc_core::ErrorCode::MethodNotFound,
            ));
        }
    }

    pub fn remove_vote_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.subscriptions.vote_subscriptions.write().unwrap();
        subscriptions.remove(id).is_some()
    }

    pub fn notify_vote(&self, vote: &Vote) {
        self.enqueue_notification(NotificationEntry::Vote(vote.clone()));
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
        rooted_slots.sort_unstable();
        rooted_slots.into_iter().for_each(|root| {
            self.enqueue_notification(NotificationEntry::SlotUpdate(SlotUpdate::Root {
                slot: root,
                timestamp: timestamp(),
            }));
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
    ) {
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            match notification_receiver.recv_timeout(Duration::from_millis(RECEIVE_DELAY_MILLIS)) {
                Ok(notification_entry) => match notification_entry {
                    NotificationEntry::Slot(slot_info) => {
                        let subscriptions = subscriptions.slot_subscriptions.read().unwrap();
                        let num_subscriptions = subscriptions.len();
                        if num_subscriptions > 0 {
                            debug!(
                                "slot notify: {:?}, num_subscriptions: {:?}",
                                slot_info, num_subscriptions
                            );
                        }
                        for (_, sink) in subscriptions.iter() {
                            inc_new_counter_info!("rpc-subscription-notify-slot", 1);
                            notifier.notify(slot_info, sink);
                        }
                    }
                    NotificationEntry::SlotUpdate(slot_update) => {
                        let subscriptions =
                            subscriptions.slots_updates_subscriptions.read().unwrap();
                        let slot_update = Arc::new(slot_update);
                        for (_, sink) in subscriptions.iter() {
                            inc_new_counter_info!("rpc-subscription-notify-slots-updates", 1);
                            notifier.notify(slot_update.clone(), sink);
                        }
                    }
                    // These notifications are only triggered by votes observed on gossip,
                    // unlike `NotificationEntry::Gossip`, which also accounts for slots seen
                    // in VoteState's from bank states built in ReplayStage.
                    NotificationEntry::Vote(ref vote_info) => {
                        let subscriptions = subscriptions.vote_subscriptions.read().unwrap();
                        let num_subscriptions = subscriptions.len();
                        if num_subscriptions > 0 {
                            debug!(
                                "vote notify: {:?}, num_subscriptions: {:?}",
                                vote_info, num_subscriptions
                            );
                        }
                        for (_, sink) in subscriptions.iter() {
                            inc_new_counter_info!("rpc-subscription-notify-vote", 1);
                            notifier.notify(
                                RpcVote {
                                    // TODO: Remove clones
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
                        let num_subscriptions = subscriptions.len();
                        if num_subscriptions > 0 {
                            debug!(
                                "root notify: {:?}, num_subscriptions: {:?}",
                                root, num_subscriptions
                            );
                        }
                        for (_, sink) in subscriptions.iter() {
                            inc_new_counter_info!("rpc-subscription-notify-root", 1);
                            notifier.notify(root, sink);
                        }
                    }
                    NotificationEntry::Bank(commitment_slots) => {
                        RpcSubscriptions::notify_accounts_logs_programs_signatures(
                            &subscriptions.account_subscriptions,
                            &subscriptions.logs_subscriptions,
                            &subscriptions.program_subscriptions,
                            &subscriptions.signature_subscriptions,
                            &bank_forks,
                            &commitment_slots,
                            &notifier,
                            "bank",
                        )
                    }
                    NotificationEntry::Gossip(slot) => {
                        Self::process_gossip_notification(
                            slot,
                            &notifier,
                            &subscriptions,
                            &bank_forks,
                        );
                    }
                    NotificationEntry::SignaturesReceived(slot_signatures) => {
                        RpcSubscriptions::process_signatures_received(
                            &slot_signatures,
                            &subscriptions.gossip_signature_subscriptions,
                            &notifier,
                        );
                        RpcSubscriptions::process_signatures_received(
                            &slot_signatures,
                            &subscriptions.signature_subscriptions,
                            &notifier,
                        );
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
    ) {
        let commitment_slots = CommitmentSlots {
            highest_confirmed_slot: slot,
            ..CommitmentSlots::default()
        };
        RpcSubscriptions::notify_accounts_logs_programs_signatures(
            &subscriptions.gossip_account_subscriptions,
            &subscriptions.gossip_logs_subscriptions,
            &subscriptions.gossip_program_subscriptions,
            &subscriptions.gossip_signature_subscriptions,
            bank_forks,
            &commitment_slots,
            notifier,
            "gossip",
        );
    }

    fn notify_accounts_logs_programs_signatures(
        account_subscriptions: &Arc<RpcAccountSubscriptions>,
        logs_subscriptions: &Arc<RpcLogsSubscriptions>,
        program_subscriptions: &Arc<RpcProgramSubscriptions>,
        signature_subscriptions: &Arc<RpcSignatureSubscriptions>,
        bank_forks: &Arc<RwLock<BankForks>>,
        commitment_slots: &CommitmentSlots,
        notifier: &RpcNotifier,
        source: &'static str,
    ) {
        let mut accounts_time = Measure::start("accounts");
        let pubkeys: Vec<_> = {
            let subs = account_subscriptions.read().unwrap();
            subs.keys().cloned().collect()
        };
        let mut num_pubkeys_notified = 0;
        for pubkey in &pubkeys {
            num_pubkeys_notified += Self::check_account(
                pubkey,
                bank_forks,
                account_subscriptions.clone(),
                notifier,
                commitment_slots,
            )
            .len();
        }
        accounts_time.stop();

        let mut logs_time = Measure::start("logs");
        let logs: Vec<_> = {
            let subs = logs_subscriptions.read().unwrap();
            subs.keys().cloned().collect()
        };
        let mut num_logs_notified = 0;
        for address in &logs {
            num_logs_notified += Self::check_logs(
                address,
                bank_forks,
                logs_subscriptions.clone(),
                notifier,
                commitment_slots,
            )
            .len();
        }
        logs_time.stop();

        let mut programs_time = Measure::start("programs");
        let programs: Vec<_> = {
            let subs = program_subscriptions.read().unwrap();
            subs.keys().cloned().collect()
        };
        let mut num_programs_notified = 0;
        for program_id in &programs {
            num_programs_notified += Self::check_program(
                program_id,
                bank_forks,
                program_subscriptions.clone(),
                notifier,
                commitment_slots,
            )
            .len();
        }
        programs_time.stop();

        let mut signatures_time = Measure::start("signatures");
        let signatures: Vec<_> = {
            let subs = signature_subscriptions.read().unwrap();
            subs.keys().cloned().collect()
        };
        let mut num_signatures_notified = 0;
        for signature in &signatures {
            num_signatures_notified += Self::check_signature(
                signature,
                bank_forks,
                signature_subscriptions.clone(),
                notifier,
                commitment_slots,
            )
            .len();
        }
        signatures_time.stop();
        let total_notified = num_pubkeys_notified + num_programs_notified + num_signatures_notified;
        let total_ms = accounts_time.as_ms() + programs_time.as_ms() + signatures_time.as_ms();
        if total_notified > 0 || total_ms > 10 {
            debug!(
                "notified({}): accounts: {} / {} ({}) programs: {} / {} ({}) signatures: {} / {} ({})",
                source,
                pubkeys.len(),
                num_pubkeys_notified,
                accounts_time,
                programs.len(),
                num_programs_notified,
                programs_time,
                signatures.len(),
                num_signatures_notified,
                signatures_time,
            );
            inc_new_counter_info!("rpc-subscription-notify-bank-or-gossip", total_notified);
            datapoint_info!(
                "rpc_subscriptions",
                ("source", source.to_string(), String),
                ("num_account_subscriptions", pubkeys.len(), i64),
                ("num_account_pubkeys_notified", num_pubkeys_notified, i64),
                ("accounts_time", accounts_time.as_us() as i64, i64),
                ("num_logs_subscriptions", logs.len(), i64),
                ("num_logs_notified", num_logs_notified, i64),
                ("logs_time", logs_time.as_us() as i64, i64),
                ("num_program_subscriptions", programs.len(), i64),
                ("num_programs_notified", num_programs_notified, i64),
                ("programs_time", programs_time.as_us() as i64, i64),
                ("num_signature_subscriptions", signatures.len(), i64),
                ("num_signatures_notified", num_signatures_notified, i64),
                ("signatures_time", signatures_time.as_us() as i64, i64)
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
                            sink,
                        );
                    }
                }
            }
        }
    }

    fn shutdown(&mut self) -> std::thread::Result<()> {
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
    use {
        super::*,
        crate::optimistically_confirmed_bank_tracker::{
            BankNotification, OptimisticallyConfirmedBank, OptimisticallyConfirmedBankTracker,
        },
        jsonrpc_core::futures::StreamExt,
        jsonrpc_pubsub::typed::Subscriber,
        serial_test::serial,
        solana_runtime::{
            commitment::BlockCommitment,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
        },
        solana_sdk::{
            message::Message,
            signature::{Keypair, Signer},
            stake, system_instruction, system_program, system_transaction,
            transaction::Transaction,
        },
        std::{
            fmt::Debug,
            sync::{atomic::Ordering::Relaxed, mpsc::channel},
        },
        tokio::{
            runtime::Runtime,
            time::{sleep, timeout},
        },
    };

    pub(crate) fn robust_poll_or_panic<T: Debug + Send + 'static>(
        receiver: jsonrpc_core::futures::channel::mpsc::UnboundedReceiver<T>,
    ) -> (
        T,
        jsonrpc_core::futures::channel::mpsc::UnboundedReceiver<T>,
    ) {
        let (inner_sender, inner_receiver) = channel();
        let rt = Runtime::new().unwrap();
        rt.spawn(async move {
            let result = timeout(
                Duration::from_millis(RECEIVE_DELAY_MILLIS),
                receiver.into_future(),
            )
            .await
            .unwrap_or_else(|err| panic!("stream error {:?}", err));

            match result {
                (Some(value), receiver) => {
                    inner_sender.send((value, receiver)).expect("send error")
                }
                (None, _) => panic!("unexpected end of stream"),
            }

            sleep(Duration::from_millis(RECEIVE_DELAY_MILLIS * 2)).await;
        });
        inner_receiver.recv().expect("recv error")
    }

    fn make_account_result(lamports: u64, subscription: u64, data: &str) -> serde_json::Value {
        json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "context": { "slot": 1 },
                   "value": {
                       "data": data,
                       "executable": false,
                       "lamports": lamports,
                       "owner": "11111111111111111111111111111111",
                       "rentEpoch": 0,
                    },
               },
               "subscription": subscription,
           }
        })
    }

    #[test]
    #[serial]
    fn test_check_account_subscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new_for_tests(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let bank0 = bank_forks.read().unwrap().get(0).unwrap().clone();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);
        let alice = Keypair::new();

        let exit = Arc::new(AtomicBool::new(false));
        let subscriptions = RpcSubscriptions::new(
            &exit,
            bank_forks.clone(),
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests_with_slots(
                1, 1,
            ))),
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
        );

        let tx0 = system_transaction::create_account(
            &mint_keypair,
            &alice,
            blockhash,
            1,
            0,
            &system_program::id(),
        );
        let expected0 = make_account_result(1, 0, "");

        let tx1 = {
            let instruction =
                system_instruction::transfer(&alice.pubkey(), &mint_keypair.pubkey(), 1);
            let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
            Transaction::new(&[&alice, &mint_keypair], message, blockhash)
        };
        let expected1 = make_account_result(0, 1, "");

        let tx2 = system_transaction::create_account(
            &mint_keypair,
            &alice,
            blockhash,
            1,
            1024,
            &system_program::id(),
        );
        let expected2 = make_account_result(1, 2, "error: data too large for bs58 encoding");

        let subscribe_cases = vec![
            (alice.pubkey(), tx0, expected0),
            (alice.pubkey(), tx1, expected1),
            (alice.pubkey(), tx2, expected2),
        ];

        for (i, (pubkey, tx, expected)) in subscribe_cases.iter().enumerate() {
            let (sub, _id_receiver, recv) = Subscriber::new_test("accountNotification");
            let sub_id = SubscriptionId::Number(i as u64);
            subscriptions.add_account_subscription(
                *pubkey,
                Some(RpcAccountInfoConfig {
                    commitment: Some(CommitmentConfig::processed()),
                    encoding: None,
                    data_slice: None,
                }),
                sub_id.clone(),
                sub,
            );

            assert!(subscriptions
                .subscriptions
                .account_subscriptions
                .read()
                .unwrap()
                .contains_key(pubkey));

            bank_forks
                .read()
                .unwrap()
                .get(1)
                .unwrap()
                .process_transaction(tx)
                .unwrap();
            let commitment_slots = CommitmentSlots {
                slot: 1,
                ..CommitmentSlots::default()
            };
            subscriptions.notify_subscribers(commitment_slots);
            let (response, _) = robust_poll_or_panic(recv);

            assert_eq!(serde_json::to_string(&expected).unwrap(), response);
            subscriptions.remove_account_subscription(&sub_id);

            assert!(!subscriptions
                .subscriptions
                .account_subscriptions
                .read()
                .unwrap()
                .contains_key(pubkey));
        }
    }

    #[test]
    #[serial]
    fn test_check_program_subscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new_for_tests(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let alice = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &alice,
            blockhash,
            1,
            16,
            &stake::program::id(),
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
        let sub_id = SubscriptionId::Number(0);
        let exit = Arc::new(AtomicBool::new(false));
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let subscriptions = RpcSubscriptions::new(
            &exit,
            bank_forks,
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
            optimistically_confirmed_bank,
        );
        subscriptions.add_program_subscription(
            stake::program::id(),
            Some(RpcProgramAccountsConfig {
                account_config: RpcAccountInfoConfig {
                    commitment: Some(CommitmentConfig::processed()),
                    ..RpcAccountInfoConfig::default()
                },
                ..RpcProgramAccountsConfig::default()
            }),
            sub_id.clone(),
            subscriber,
        );

        assert!(subscriptions
            .subscriptions
            .program_subscriptions
            .read()
            .unwrap()
            .contains_key(&stake::program::id()));

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
            .contains_key(&stake::program::id()));
    }

    #[test]
    #[serial]
    fn test_check_program_subscribe_for_missing_optimistically_confirmed_slot() {
        // Testing if we can get the pubsub notification if a slot does not
        // receive OptimisticallyConfirmed but its descendant slot get the confirmed
        // notification.
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new_for_tests(&genesis_config);
        bank.lazy_rent_collection.store(true, Relaxed);

        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));

        let bank0 = bank_forks.read().unwrap().get(0).unwrap().clone();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);
        let bank1 = bank_forks.read().unwrap().get(1).unwrap().clone();

        // add account for alice and process the transaction at bank1
        let alice = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &alice,
            blockhash,
            1,
            16,
            &stake::program::id(),
        );

        bank1.process_transaction(&tx).unwrap();

        let bank2 = Bank::new_from_parent(&bank1, &Pubkey::default(), 2);
        bank_forks.write().unwrap().insert(bank2);

        // add account for bob and process the transaction at bank2
        let bob = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &bob,
            blockhash,
            2,
            16,
            &stake::program::id(),
        );
        let bank2 = bank_forks.read().unwrap().get(2).unwrap().clone();

        bank2.process_transaction(&tx).unwrap();

        let bank3 = Bank::new_from_parent(&bank2, &Pubkey::default(), 3);
        bank_forks.write().unwrap().insert(bank3);

        // add account for joe and process the transaction at bank3
        let joe = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &joe,
            blockhash,
            3,
            16,
            &stake::program::id(),
        );
        let bank3 = bank_forks.read().unwrap().get(3).unwrap().clone();

        bank3.process_transaction(&tx).unwrap();

        // now add programSubscribe at the "confirmed" commitment level
        let (subscriber, _id_receiver, transport_receiver) =
            Subscriber::new_test("programNotification");
        let sub_id = SubscriptionId::Number(0);
        let exit = Arc::new(AtomicBool::new(false));
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let mut pending_optimistically_confirmed_banks = HashSet::new();

        let subscriptions = Arc::new(RpcSubscriptions::new(
            &exit,
            bank_forks.clone(),
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests_with_slots(
                1, 1,
            ))),
            optimistically_confirmed_bank.clone(),
        ));
        subscriptions.add_program_subscription(
            stake::program::id(),
            Some(RpcProgramAccountsConfig {
                account_config: RpcAccountInfoConfig {
                    commitment: Some(CommitmentConfig::confirmed()),
                    ..RpcAccountInfoConfig::default()
                },
                ..RpcProgramAccountsConfig::default()
            }),
            sub_id.clone(),
            subscriber,
        );

        assert!(subscriptions
            .subscriptions
            .gossip_program_subscriptions
            .read()
            .unwrap()
            .contains_key(&stake::program::id()));

        let mut highest_confirmed_slot: Slot = 0;
        let mut last_notified_confirmed_slot: Slot = 0;
        // Optimistically notifying slot 3 without notifying slot 1 and 2, bank3 is unfrozen, we expect
        // to see transaction for alice and bob to be notified in order.
        OptimisticallyConfirmedBankTracker::process_notification(
            BankNotification::OptimisticallyConfirmed(3),
            &bank_forks,
            &optimistically_confirmed_bank,
            &subscriptions,
            &mut pending_optimistically_confirmed_banks,
            &mut last_notified_confirmed_slot,
            &mut highest_confirmed_slot,
        );

        // a closure to reduce code duplications in building expected responses:
        let build_expected_resp = |slot: Slot, lamports: u64, pubkey: &str, subscription: i32| {
            json!({
               "jsonrpc": "2.0",
               "method": "programNotification",
               "params": {
                   "result": {
                       "context": { "slot": slot },
                       "value": {
                           "account": {
                              "data": "1111111111111111",
                              "executable": false,
                              "lamports": lamports,
                              "owner": "Stake11111111111111111111111111111111111111",
                              "rentEpoch": 0,
                           },
                           "pubkey": pubkey,
                        },
                   },
                   "subscription": subscription,
               }
            })
        };

        let (response, transport_receiver) = robust_poll_or_panic(transport_receiver);
        let expected = build_expected_resp(1, 1, &alice.pubkey().to_string(), 0);
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);

        let (response, transport_receiver) = robust_poll_or_panic(transport_receiver);
        let expected = build_expected_resp(2, 2, &bob.pubkey().to_string(), 0);
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);

        bank3.freeze();
        OptimisticallyConfirmedBankTracker::process_notification(
            BankNotification::Frozen(bank3),
            &bank_forks,
            &optimistically_confirmed_bank,
            &subscriptions,
            &mut pending_optimistically_confirmed_banks,
            &mut last_notified_confirmed_slot,
            &mut highest_confirmed_slot,
        );

        let (response, _) = robust_poll_or_panic(transport_receiver);
        let expected = build_expected_resp(3, 3, &joe.pubkey().to_string(), 0);
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);
        subscriptions.remove_program_subscription(&sub_id);
    }

    #[test]
    #[serial]
    #[should_panic]
    fn test_check_program_subscribe_for_missing_optimistically_confirmed_slot_with_no_banks_no_notifications(
    ) {
        // Testing if we can get the pubsub notification if a slot does not
        // receive OptimisticallyConfirmed but its descendant slot get the confirmed
        // notification with a bank in the BankForks. We are not expecting to receive any notifications -- should panic.
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new_for_tests(&genesis_config);
        bank.lazy_rent_collection.store(true, Relaxed);

        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));

        let bank0 = bank_forks.read().unwrap().get(0).unwrap().clone();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);
        let bank1 = bank_forks.read().unwrap().get(1).unwrap().clone();

        // add account for alice and process the transaction at bank1
        let alice = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &alice,
            blockhash,
            1,
            16,
            &stake::program::id(),
        );

        bank1.process_transaction(&tx).unwrap();

        let bank2 = Bank::new_from_parent(&bank1, &Pubkey::default(), 2);
        bank_forks.write().unwrap().insert(bank2);

        // add account for bob and process the transaction at bank2
        let bob = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &bob,
            blockhash,
            2,
            16,
            &stake::program::id(),
        );
        let bank2 = bank_forks.read().unwrap().get(2).unwrap().clone();

        bank2.process_transaction(&tx).unwrap();

        // now add programSubscribe at the "confirmed" commitment level
        let (subscriber, _id_receiver, transport_receiver) =
            Subscriber::new_test("programNotification");
        let sub_id = SubscriptionId::Number(0);
        let exit = Arc::new(AtomicBool::new(false));
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let mut pending_optimistically_confirmed_banks = HashSet::new();

        let subscriptions = Arc::new(RpcSubscriptions::new(
            &exit,
            bank_forks.clone(),
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests_with_slots(
                1, 1,
            ))),
            optimistically_confirmed_bank.clone(),
        ));
        subscriptions.add_program_subscription(
            stake::program::id(),
            Some(RpcProgramAccountsConfig {
                account_config: RpcAccountInfoConfig {
                    commitment: Some(CommitmentConfig::confirmed()),
                    ..RpcAccountInfoConfig::default()
                },
                ..RpcProgramAccountsConfig::default()
            }),
            sub_id,
            subscriber,
        );

        assert!(subscriptions
            .subscriptions
            .gossip_program_subscriptions
            .read()
            .unwrap()
            .contains_key(&stake::program::id()));

        let mut highest_confirmed_slot: Slot = 0;
        let mut last_notified_confirmed_slot: Slot = 0;
        // Optimistically notifying slot 3 without notifying slot 1 and 2, bank3 is not in the bankforks, we do not
        // expect to see any RPC notifications.
        OptimisticallyConfirmedBankTracker::process_notification(
            BankNotification::OptimisticallyConfirmed(3),
            &bank_forks,
            &optimistically_confirmed_bank,
            &subscriptions,
            &mut pending_optimistically_confirmed_banks,
            &mut last_notified_confirmed_slot,
            &mut highest_confirmed_slot,
        );

        // The following should panic
        let (_response, _transport_receiver) = robust_poll_or_panic(transport_receiver);
    }

    #[test]
    #[serial]
    fn test_check_program_subscribe_for_missing_optimistically_confirmed_slot_with_no_banks() {
        // Testing if we can get the pubsub notification if a slot does not
        // receive OptimisticallyConfirmed but its descendant slot get the confirmed
        // notification. It differs from the test_check_program_subscribe_for_missing_optimistically_confirmed_slot
        // test in that when the descendant get confirmed, the descendant does not have a bank yet.
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new_for_tests(&genesis_config);
        bank.lazy_rent_collection.store(true, Relaxed);

        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));

        let bank0 = bank_forks.read().unwrap().get(0).unwrap().clone();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);
        let bank1 = bank_forks.read().unwrap().get(1).unwrap().clone();

        // add account for alice and process the transaction at bank1
        let alice = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &alice,
            blockhash,
            1,
            16,
            &stake::program::id(),
        );

        bank1.process_transaction(&tx).unwrap();

        let bank2 = Bank::new_from_parent(&bank1, &Pubkey::default(), 2);
        bank_forks.write().unwrap().insert(bank2);

        // add account for bob and process the transaction at bank2
        let bob = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &bob,
            blockhash,
            2,
            16,
            &stake::program::id(),
        );
        let bank2 = bank_forks.read().unwrap().get(2).unwrap().clone();

        bank2.process_transaction(&tx).unwrap();

        // now add programSubscribe at the "confirmed" commitment level
        let (subscriber, _id_receiver, transport_receiver) =
            Subscriber::new_test("programNotification");
        let sub_id = SubscriptionId::Number(0);
        let exit = Arc::new(AtomicBool::new(false));
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let mut pending_optimistically_confirmed_banks = HashSet::new();

        let subscriptions = Arc::new(RpcSubscriptions::new(
            &exit,
            bank_forks.clone(),
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests_with_slots(
                1, 1,
            ))),
            optimistically_confirmed_bank.clone(),
        ));
        subscriptions.add_program_subscription(
            stake::program::id(),
            Some(RpcProgramAccountsConfig {
                account_config: RpcAccountInfoConfig {
                    commitment: Some(CommitmentConfig::confirmed()),
                    ..RpcAccountInfoConfig::default()
                },
                ..RpcProgramAccountsConfig::default()
            }),
            sub_id.clone(),
            subscriber,
        );

        assert!(subscriptions
            .subscriptions
            .gossip_program_subscriptions
            .read()
            .unwrap()
            .contains_key(&stake::program::id()));

        let mut highest_confirmed_slot: Slot = 0;
        let mut last_notified_confirmed_slot: Slot = 0;
        // Optimistically notifying slot 3 without notifying slot 1 and 2, bank3 is not in the bankforks, we expect
        // to see transaction for alice and bob to be notified only when bank3 is added to the fork and
        // frozen. The notifications should be in the increasing order of the slot.
        OptimisticallyConfirmedBankTracker::process_notification(
            BankNotification::OptimisticallyConfirmed(3),
            &bank_forks,
            &optimistically_confirmed_bank,
            &subscriptions,
            &mut pending_optimistically_confirmed_banks,
            &mut last_notified_confirmed_slot,
            &mut highest_confirmed_slot,
        );

        // a closure to reduce code duplications in building expected responses:
        let build_expected_resp = |slot: Slot, lamports: u64, pubkey: &str, subscription: i32| {
            json!({
               "jsonrpc": "2.0",
               "method": "programNotification",
               "params": {
                   "result": {
                       "context": { "slot": slot },
                       "value": {
                           "account": {
                              "data": "1111111111111111",
                              "executable": false,
                              "lamports": lamports,
                              "owner": "Stake11111111111111111111111111111111111111",
                              "rentEpoch": 0,
                           },
                           "pubkey": pubkey,
                        },
                   },
                   "subscription": subscription,
               }
            })
        };

        let bank3 = Bank::new_from_parent(&bank2, &Pubkey::default(), 3);
        bank_forks.write().unwrap().insert(bank3);

        // add account for joe and process the transaction at bank3
        let joe = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &joe,
            blockhash,
            3,
            16,
            &stake::program::id(),
        );
        let bank3 = bank_forks.read().unwrap().get(3).unwrap().clone();

        bank3.process_transaction(&tx).unwrap();
        bank3.freeze();
        OptimisticallyConfirmedBankTracker::process_notification(
            BankNotification::Frozen(bank3),
            &bank_forks,
            &optimistically_confirmed_bank,
            &subscriptions,
            &mut pending_optimistically_confirmed_banks,
            &mut last_notified_confirmed_slot,
            &mut highest_confirmed_slot,
        );

        let (response, transport_receiver) = robust_poll_or_panic(transport_receiver);
        let expected = build_expected_resp(1, 1, &alice.pubkey().to_string(), 0);
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);

        let (response, transport_receiver) = robust_poll_or_panic(transport_receiver);
        let expected = build_expected_resp(2, 2, &bob.pubkey().to_string(), 0);
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);

        let (response, _) = robust_poll_or_panic(transport_receiver);
        let expected = build_expected_resp(3, 3, &joe.pubkey().to_string(), 0);
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);
        subscriptions.remove_program_subscription(&sub_id);
    }

    #[test]
    #[serial]
    fn test_check_signature_subscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new_for_tests(&genesis_config);
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

        let next_bank = Bank::new_from_parent(
            &bank_forks.get(0).unwrap().clone(),
            &solana_sdk::pubkey::new_rand(),
            1,
        );
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
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let subscriptions = RpcSubscriptions::new(
            &exit,
            bank_forks,
            Arc::new(RwLock::new(block_commitment_cache)),
            optimistically_confirmed_bank,
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
                commitment: Some(CommitmentConfig::processed()),
                enable_received_notification: Some(false),
            }),
            SubscriptionId::Number(1),
            past_bank_sub1,
        );
        subscriptions.add_signature_subscription(
            past_bank_tx.signatures[0],
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::finalized()),
                enable_received_notification: Some(false),
            }),
            SubscriptionId::Number(2),
            past_bank_sub2,
        );
        subscriptions.add_signature_subscription(
            processed_tx.signatures[0],
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::processed()),
                enable_received_notification: Some(false),
            }),
            SubscriptionId::Number(3),
            processed_sub,
        );
        subscriptions.add_signature_subscription(
            unprocessed_tx.signatures[0],
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::processed()),
                enable_received_notification: Some(false),
            }),
            SubscriptionId::Number(4),
            Subscriber::new_test("signatureNotification").0,
        );
        // Add a subscription that gets `received` notifications
        subscriptions.add_signature_subscription(
            unprocessed_tx.signatures[0],
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::processed()),
                enable_received_notification: Some(true),
            }),
            SubscriptionId::Number(5),
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
        let sub_id = SubscriptionId::Number(0);
        let exit = Arc::new(AtomicBool::new(false));
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let subscriptions = RpcSubscriptions::new(
            &exit,
            bank_forks,
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
            optimistically_confirmed_bank,
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
        let sub_id = SubscriptionId::Number(0);
        let exit = Arc::new(AtomicBool::new(false));
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let subscriptions = RpcSubscriptions::new(
            &exit,
            bank_forks,
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
            optimistically_confirmed_bank,
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
        let commitment = CommitmentConfig::confirmed();

        let num_keys = 5;
        for key in 0..num_keys {
            let (subscriber, _id_receiver, _transport_receiver) =
                Subscriber::new_test("notification");
            let sub_id = SubscriptionId::Number(key);
            add_subscription(
                &mut subscriptions,
                key,
                commitment,
                sub_id,
                subscriber,
                0,
                None,
            );
        }

        // Add another subscription to the "0" key
        let (subscriber, _id_receiver, _transport_receiver) = Subscriber::new_test("notification");
        let extra_sub_id = SubscriptionId::Number(num_keys);
        add_subscription(
            &mut subscriptions,
            0,
            commitment,
            extra_sub_id.clone(),
            subscriber,
            0,
            None,
        );

        assert_eq!(subscriptions.len(), num_keys as usize);
        assert_eq!(subscriptions.get(&0).unwrap().len(), 2);
        assert_eq!(subscriptions.get(&1).unwrap().len(), 1);

        assert!(remove_subscription(
            &mut subscriptions,
            &SubscriptionId::Number(0)
        ));
        assert_eq!(subscriptions.len(), num_keys as usize);
        assert_eq!(subscriptions.get(&0).unwrap().len(), 1);
        assert!(!remove_subscription(
            &mut subscriptions,
            &SubscriptionId::Number(0)
        ));

        assert!(remove_subscription(&mut subscriptions, &extra_sub_id));
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
        let bank = Bank::new_for_tests(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let bank0 = bank_forks.read().unwrap().get(0).unwrap().clone();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);
        let bank2 = Bank::new_from_parent(&bank0, &Pubkey::default(), 2);
        bank_forks.write().unwrap().insert(bank2);
        let alice = Keypair::new();

        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let mut pending_optimistically_confirmed_banks = HashSet::new();

        let (subscriber0, _id_receiver, transport_receiver0) =
            Subscriber::new_test("accountNotification");
        let (subscriber1, _id_receiver, transport_receiver1) =
            Subscriber::new_test("accountNotification");
        let exit = Arc::new(AtomicBool::new(false));
        let subscriptions = Arc::new(RpcSubscriptions::new(
            &exit,
            bank_forks.clone(),
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests_with_slots(
                1, 1,
            ))),
            optimistically_confirmed_bank.clone(),
        ));
        let sub_id0 = SubscriptionId::Number(0);
        subscriptions.add_account_subscription(
            alice.pubkey(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::confirmed()),
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
            &stake::program::id(),
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
        let mut highest_confirmed_slot: Slot = 0;
        let mut last_notified_confirmed_slot: Slot = 0;
        OptimisticallyConfirmedBankTracker::process_notification(
            BankNotification::OptimisticallyConfirmed(2),
            &bank_forks,
            &optimistically_confirmed_bank,
            &subscriptions,
            &mut pending_optimistically_confirmed_banks,
            &mut last_notified_confirmed_slot,
            &mut highest_confirmed_slot,
        );

        // Now, notify the frozen bank and ensure its notifications are processed
        highest_confirmed_slot = 0;
        OptimisticallyConfirmedBankTracker::process_notification(
            BankNotification::OptimisticallyConfirmed(1),
            &bank_forks,
            &optimistically_confirmed_bank,
            &subscriptions,
            &mut pending_optimistically_confirmed_banks,
            &mut last_notified_confirmed_slot,
            &mut highest_confirmed_slot,
        );

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

        let sub_id1 = SubscriptionId::Number(1);
        subscriptions.add_account_subscription(
            alice.pubkey(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::confirmed()),
                encoding: None,
                data_slice: None,
            }),
            sub_id1.clone(),
            subscriber1,
        );

        let bank2 = bank_forks.read().unwrap().get(2).unwrap().clone();
        bank2.freeze();
        highest_confirmed_slot = 0;
        OptimisticallyConfirmedBankTracker::process_notification(
            BankNotification::Frozen(bank2),
            &bank_forks,
            &optimistically_confirmed_bank,
            &subscriptions,
            &mut pending_optimistically_confirmed_banks,
            &mut last_notified_confirmed_slot,
            &mut highest_confirmed_slot,
        );
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

    #[test]
    fn test_total_nested_subscriptions() {
        let mock_subscriptions = RwLock::new(HashMap::new());
        assert_eq!(total_nested_subscriptions(&mock_subscriptions), 0);

        mock_subscriptions
            .write()
            .unwrap()
            .insert(0, HashMap::new());
        assert_eq!(total_nested_subscriptions(&mock_subscriptions), 0);

        mock_subscriptions
            .write()
            .unwrap()
            .entry(0)
            .and_modify(|map| {
                map.insert(0, "test");
            });
        assert_eq!(total_nested_subscriptions(&mock_subscriptions), 1);

        mock_subscriptions
            .write()
            .unwrap()
            .entry(0)
            .and_modify(|map| {
                map.insert(1, "test");
            });
        assert_eq!(total_nested_subscriptions(&mock_subscriptions), 2);

        mock_subscriptions
            .write()
            .unwrap()
            .insert(1, HashMap::new());
        assert_eq!(total_nested_subscriptions(&mock_subscriptions), 2);

        mock_subscriptions
            .write()
            .unwrap()
            .entry(1)
            .and_modify(|map| {
                map.insert(0, "test");
            });
        assert_eq!(total_nested_subscriptions(&mock_subscriptions), 3);
    }

    #[test]
    fn test_total_subscriptions() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(100);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let subscriptions = RpcSubscriptions::default_with_bank_forks(bank_forks);

        let (subscriber, _id_receiver, _transport_receiver) =
            Subscriber::new_test("accountNotification");
        let account_sub_id = SubscriptionId::Number(0u64);
        subscriptions.add_account_subscription(
            Pubkey::default(),
            None,
            account_sub_id.clone(),
            subscriber,
        );
        assert_eq!(subscriptions.total(), 1);

        let (subscriber, _id_receiver, _transport_receiver) =
            Subscriber::new_test("programNotification");
        let program_sub_id = SubscriptionId::Number(1u64);
        subscriptions.add_program_subscription(
            Pubkey::default(),
            None,
            program_sub_id.clone(),
            subscriber,
        );
        assert_eq!(subscriptions.total(), 2);

        let (subscriber, _id_receiver, _transport_receiver) =
            Subscriber::new_test("logsNotification");
        let logs_sub_id = SubscriptionId::Number(2u64);
        subscriptions.add_logs_subscription(None, false, None, logs_sub_id.clone(), subscriber);
        assert_eq!(subscriptions.total(), 3);

        let (subscriber, _id_receiver, _transport_receiver) =
            Subscriber::new_test("signatureNotification");
        let sig_sub_id = SubscriptionId::Number(3u64);
        subscriptions.add_signature_subscription(
            Signature::default(),
            None,
            sig_sub_id.clone(),
            subscriber,
        );
        assert_eq!(subscriptions.total(), 4);

        let (subscriber, _id_receiver, _transport_receiver) =
            Subscriber::new_test("slotNotification");
        let slot_sub_id = SubscriptionId::Number(4u64);
        subscriptions.add_slot_subscription(slot_sub_id.clone(), subscriber);
        assert_eq!(subscriptions.total(), 5);

        let (subscriber, _id_receiver, _transport_receiver) =
            Subscriber::new_test("voteNotification");
        let vote_sub_id = SubscriptionId::Number(5u64);
        subscriptions.add_vote_subscription(vote_sub_id.clone(), subscriber);
        assert_eq!(subscriptions.total(), 6);

        let (subscriber, _id_receiver, _transport_receiver) =
            Subscriber::new_test("rootNotification");
        let root_sub_id = SubscriptionId::Number(6u64);
        subscriptions.add_root_subscription(root_sub_id.clone(), subscriber);
        assert_eq!(subscriptions.total(), 7);

        // Add duplicate account subscription to ensure totals include all subscriptions on all keys
        let (subscriber, _id_receiver, _transport_receiver) =
            Subscriber::new_test("accountNotification2");
        let account_dupe_sub_id = SubscriptionId::Number(7u64);
        subscriptions.add_account_subscription(
            Pubkey::default(),
            None,
            account_dupe_sub_id.clone(),
            subscriber,
        );
        assert_eq!(subscriptions.total(), 8);

        subscriptions.remove_account_subscription(&account_sub_id);
        assert_eq!(subscriptions.total(), 7);

        subscriptions.remove_account_subscription(&account_dupe_sub_id);
        assert_eq!(subscriptions.total(), 6);

        subscriptions.remove_program_subscription(&program_sub_id);
        assert_eq!(subscriptions.total(), 5);

        subscriptions.remove_logs_subscription(&logs_sub_id);
        assert_eq!(subscriptions.total(), 4);

        subscriptions.remove_signature_subscription(&sig_sub_id);
        assert_eq!(subscriptions.total(), 3);

        subscriptions.remove_slot_subscription(&slot_sub_id);
        assert_eq!(subscriptions.total(), 2);

        subscriptions.remove_vote_subscription(&vote_sub_id);
        assert_eq!(subscriptions.total(), 1);

        subscriptions.remove_root_subscription(&root_sub_id);
        assert_eq!(subscriptions.total(), 0);
    }
}

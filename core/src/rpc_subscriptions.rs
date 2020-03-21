//! The `pubsub` module implements a threaded subscription service on client RPC request

use core::hash::Hash;
use jsonrpc_core::futures::Future;
use jsonrpc_pubsub::{typed::Sink, SubscriptionId};
use serde::Serialize;
use solana_client::rpc_response::{Response, RpcAccount, RpcKeyedAccount, RpcResponseContext};
use solana_ledger::bank_forks::BankForks;
use solana_runtime::bank::Bank;
use solana_sdk::{
    account::Account, clock::Slot, pubkey::Pubkey, signature::Signature, transaction,
};
use solana_vote_program::vote_state::MAX_LOCKOUT_HISTORY;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SendError, Sender};
use std::thread::{Builder, JoinHandle};
use std::time::Duration;
use std::{
    collections::{HashMap, HashSet},
    iter,
    sync::{Arc, Mutex, RwLock},
};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime, TaskExecutor};

const RECEIVE_DELAY_MILLIS: u64 = 100;

pub type Confirmations = usize;

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct SlotInfo {
    pub slot: Slot,
    pub parent: Slot,
    pub root: Slot,
}

enum NotificationEntry {
    Slot(SlotInfo),
    Bank((Slot, Arc<RwLock<BankForks>>)),
}

impl std::fmt::Debug for NotificationEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            NotificationEntry::Slot(slot_info) => write!(f, "Slot({:?})", slot_info),
            NotificationEntry::Bank((current_slot, _)) => {
                write!(f, "Bank({{current_slot: {:?}}})", current_slot)
            }
        }
    }
}

type RpcAccountSubscriptions =
    RwLock<HashMap<Pubkey, HashMap<SubscriptionId, (Sink<Response<RpcAccount>>, Confirmations)>>>;
type RpcProgramSubscriptions = RwLock<
    HashMap<Pubkey, HashMap<SubscriptionId, (Sink<Response<RpcKeyedAccount>>, Confirmations)>>,
>;
type RpcSignatureSubscriptions = RwLock<
    HashMap<
        Signature,
        HashMap<SubscriptionId, (Sink<Response<transaction::Result<()>>>, Confirmations)>,
    >,
>;
type RpcSlotSubscriptions = RwLock<HashMap<SubscriptionId, Sink<SlotInfo>>>;

fn add_subscription<K, S>(
    subscriptions: &mut HashMap<K, HashMap<SubscriptionId, (Sink<S>, Confirmations)>>,
    hashmap_key: &K,
    confirmations: Option<Confirmations>,
    sub_id: &SubscriptionId,
    sink: &Sink<S>,
) where
    K: Eq + Hash + Clone + Copy,
    S: Clone,
{
    let confirmations = confirmations.unwrap_or(0);
    let confirmations = if confirmations > MAX_LOCKOUT_HISTORY {
        MAX_LOCKOUT_HISTORY
    } else {
        confirmations
    };
    if let Some(current_hashmap) = subscriptions.get_mut(hashmap_key) {
        current_hashmap.insert(sub_id.clone(), (sink.clone(), confirmations));
        return;
    }
    let mut hashmap = HashMap::new();
    hashmap.insert(sub_id.clone(), (sink.clone(), confirmations));
    subscriptions.insert(*hashmap_key, hashmap);
}

fn remove_subscription<K, S>(
    subscriptions: &mut HashMap<K, HashMap<SubscriptionId, (Sink<S>, Confirmations)>>,
    sub_id: &SubscriptionId,
) -> bool
where
    K: Eq + Hash + Clone + Copy,
    S: Clone,
{
    let mut found = false;
    subscriptions.retain(|_, v| {
        v.retain(|k, _| {
            let retain = *k != *sub_id;
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
fn check_confirmations_and_notify<K, S, B, F, X>(
    subscriptions: &HashMap<K, HashMap<SubscriptionId, (Sink<Response<S>>, Confirmations)>>,
    hashmap_key: &K,
    current_slot: Slot,
    bank_forks: &Arc<RwLock<BankForks>>,
    bank_method: B,
    filter_results: F,
    notifier: &RpcNotifier,
) -> HashSet<SubscriptionId>
where
    K: Eq + Hash + Clone + Copy,
    S: Clone + Serialize,
    B: Fn(&Bank, &K) -> X,
    F: Fn(X, u64) -> Box<dyn Iterator<Item = S>>,
    X: Clone + Serialize,
{
    let current_ancestors = bank_forks
        .read()
        .unwrap()
        .get(current_slot)
        .unwrap()
        .ancestors
        .clone();

    let mut notified_set: HashSet<SubscriptionId> = HashSet::new();
    if let Some(hashmap) = subscriptions.get(hashmap_key) {
        for (sub_id, (sink, confirmations)) in hashmap.iter() {
            let desired_slot: Vec<u64> = current_ancestors
                .iter()
                .filter(|(_, &v)| v == *confirmations)
                .map(|(k, _)| k)
                .cloned()
                .collect();
            let root: Vec<u64> = current_ancestors
                .iter()
                .filter(|(_, &v)| v == 32)
                .map(|(k, _)| k)
                .cloned()
                .collect();
            let root = if root.len() == 1 { root[0] } else { 0 };
            if desired_slot.len() == 1 {
                let slot = desired_slot[0];
                let desired_bank = bank_forks.read().unwrap().get(slot).unwrap().clone();
                let results = bank_method(&desired_bank, hashmap_key);
                for result in filter_results(results, root) {
                    notifier.notify(
                        Response {
                            context: RpcResponseContext { slot },
                            value: result,
                        },
                        sink,
                    );
                    notified_set.insert(sub_id.clone());
                }
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
    root: Slot,
) -> Box<dyn Iterator<Item = RpcAccount>> {
    if let Some((account, fork)) = result {
        if fork >= root {
            return Box::new(iter::once(RpcAccount::encode(account)));
        }
    }
    Box::new(iter::empty())
}

fn filter_signature_result<S>(result: Option<S>, _root: Slot) -> Box<dyn Iterator<Item = S>>
where
    S: 'static + Clone + Serialize,
{
    Box::new(result.into_iter())
}

fn filter_program_results(
    accounts: Vec<(Pubkey, Account)>,
    _root: Slot,
) -> Box<dyn Iterator<Item = RpcKeyedAccount>> {
    Box::new(
        accounts
            .into_iter()
            .map(|(pubkey, account)| RpcKeyedAccount {
                pubkey: pubkey.to_string(),
                account: RpcAccount::encode(account),
            }),
    )
}

pub struct RpcSubscriptions {
    account_subscriptions: Arc<RpcAccountSubscriptions>,
    program_subscriptions: Arc<RpcProgramSubscriptions>,
    signature_subscriptions: Arc<RpcSignatureSubscriptions>,
    slot_subscriptions: Arc<RpcSlotSubscriptions>,
    notification_sender: Arc<Mutex<Sender<NotificationEntry>>>,
    t_cleanup: Option<JoinHandle<()>>,
    notifier_runtime: Option<Runtime>,
    exit: Arc<AtomicBool>,
}

impl Default for RpcSubscriptions {
    fn default() -> Self {
        Self::new(&Arc::new(AtomicBool::new(false)))
    }
}

impl Drop for RpcSubscriptions {
    fn drop(&mut self) {
        self.shutdown().unwrap_or_else(|err| {
            warn!("RPC Notification - shutdown error: {:?}", err);
        });
    }
}

impl RpcSubscriptions {
    pub fn new(exit: &Arc<AtomicBool>) -> Self {
        let (notification_sender, notification_receiver): (
            Sender<NotificationEntry>,
            Receiver<NotificationEntry>,
        ) = std::sync::mpsc::channel();

        let account_subscriptions = Arc::new(RpcAccountSubscriptions::default());
        let program_subscriptions = Arc::new(RpcProgramSubscriptions::default());
        let signature_subscriptions = Arc::new(RpcSignatureSubscriptions::default());
        let slot_subscriptions = Arc::new(RpcSlotSubscriptions::default());
        let notification_sender = Arc::new(Mutex::new(notification_sender));

        let exit_clone = exit.clone();
        let account_subscriptions_clone = account_subscriptions.clone();
        let program_subscriptions_clone = program_subscriptions.clone();
        let signature_subscriptions_clone = signature_subscriptions.clone();
        let slot_subscriptions_clone = slot_subscriptions.clone();

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
                    account_subscriptions_clone,
                    program_subscriptions_clone,
                    signature_subscriptions_clone,
                    slot_subscriptions_clone,
                );
            })
            .unwrap();

        Self {
            account_subscriptions,
            program_subscriptions,
            signature_subscriptions,
            slot_subscriptions,
            notification_sender,
            notifier_runtime: Some(notifier_runtime),
            t_cleanup: Some(t_cleanup),
            exit: exit.clone(),
        }
    }

    fn check_account(
        pubkey: &Pubkey,
        current_slot: Slot,
        bank_forks: &Arc<RwLock<BankForks>>,
        account_subscriptions: Arc<RpcAccountSubscriptions>,
        notifier: &RpcNotifier,
    ) {
        let subscriptions = account_subscriptions.read().unwrap();
        check_confirmations_and_notify(
            &subscriptions,
            pubkey,
            current_slot,
            bank_forks,
            Bank::get_account_modified_since_parent,
            filter_account_result,
            notifier,
        );
    }

    fn check_program(
        program_id: &Pubkey,
        current_slot: Slot,
        bank_forks: &Arc<RwLock<BankForks>>,
        program_subscriptions: Arc<RpcProgramSubscriptions>,
        notifier: &RpcNotifier,
    ) {
        let subscriptions = program_subscriptions.read().unwrap();
        check_confirmations_and_notify(
            &subscriptions,
            program_id,
            current_slot,
            bank_forks,
            Bank::get_program_accounts_modified_since_parent,
            filter_program_results,
            notifier,
        );
    }

    fn check_signature(
        signature: &Signature,
        current_slot: Slot,
        bank_forks: &Arc<RwLock<BankForks>>,
        signature_subscriptions: Arc<RpcSignatureSubscriptions>,
        notifier: &RpcNotifier,
    ) {
        let mut subscriptions = signature_subscriptions.write().unwrap();
        let notified_ids = check_confirmations_and_notify(
            &subscriptions,
            signature,
            current_slot,
            bank_forks,
            Bank::get_signature_status,
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
        pubkey: &Pubkey,
        confirmations: Option<Confirmations>,
        sub_id: &SubscriptionId,
        sink: &Sink<Response<RpcAccount>>,
    ) {
        let mut subscriptions = self.account_subscriptions.write().unwrap();
        add_subscription(&mut subscriptions, pubkey, confirmations, sub_id, sink);
    }

    pub fn remove_account_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.account_subscriptions.write().unwrap();
        remove_subscription(&mut subscriptions, id)
    }

    pub fn add_program_subscription(
        &self,
        program_id: &Pubkey,
        confirmations: Option<Confirmations>,
        sub_id: &SubscriptionId,
        sink: &Sink<Response<RpcKeyedAccount>>,
    ) {
        let mut subscriptions = self.program_subscriptions.write().unwrap();
        add_subscription(&mut subscriptions, program_id, confirmations, sub_id, sink);
    }

    pub fn remove_program_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.program_subscriptions.write().unwrap();
        remove_subscription(&mut subscriptions, id)
    }

    pub fn add_signature_subscription(
        &self,
        signature: &Signature,
        confirmations: Option<Confirmations>,
        sub_id: &SubscriptionId,
        sink: &Sink<Response<transaction::Result<()>>>,
    ) {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        add_subscription(&mut subscriptions, signature, confirmations, sub_id, sink);
    }

    pub fn remove_signature_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        remove_subscription(&mut subscriptions, id)
    }

    /// Notify subscribers of changes to any accounts or new signatures since
    /// the bank's last checkpoint.
    pub fn notify_subscribers(&self, current_slot: Slot, bank_forks: &Arc<RwLock<BankForks>>) {
        self.enqueue_notification(NotificationEntry::Bank((current_slot, bank_forks.clone())));
    }

    pub fn add_slot_subscription(&self, sub_id: &SubscriptionId, sink: &Sink<SlotInfo>) {
        let mut subscriptions = self.slot_subscriptions.write().unwrap();
        subscriptions.insert(sub_id.clone(), sink.clone());
    }

    pub fn remove_slot_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.slot_subscriptions.write().unwrap();
        subscriptions.remove(id).is_some()
    }

    pub fn notify_slot(&self, slot: Slot, parent: Slot, root: Slot) {
        self.enqueue_notification(NotificationEntry::Slot(SlotInfo { slot, parent, root }));
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
        account_subscriptions: Arc<RpcAccountSubscriptions>,
        program_subscriptions: Arc<RpcProgramSubscriptions>,
        signature_subscriptions: Arc<RpcSignatureSubscriptions>,
        slot_subscriptions: Arc<RpcSlotSubscriptions>,
    ) {
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            match notification_receiver.recv_timeout(Duration::from_millis(RECEIVE_DELAY_MILLIS)) {
                Ok(notification_entry) => match notification_entry {
                    NotificationEntry::Slot(slot_info) => {
                        let subscriptions = slot_subscriptions.read().unwrap();
                        for (_, sink) in subscriptions.iter() {
                            notifier.notify(slot_info, sink);
                        }
                    }
                    NotificationEntry::Bank((current_slot, bank_forks)) => {
                        let pubkeys: Vec<_> = {
                            let subs = account_subscriptions.read().unwrap();
                            subs.keys().cloned().collect()
                        };
                        for pubkey in &pubkeys {
                            Self::check_account(
                                pubkey,
                                current_slot,
                                &bank_forks,
                                account_subscriptions.clone(),
                                &notifier,
                            );
                        }

                        let programs: Vec<_> = {
                            let subs = program_subscriptions.read().unwrap();
                            subs.keys().cloned().collect()
                        };
                        for program_id in &programs {
                            Self::check_program(
                                program_id,
                                current_slot,
                                &bank_forks,
                                program_subscriptions.clone(),
                                &notifier,
                            );
                        }

                        let signatures: Vec<_> = {
                            let subs = signature_subscriptions.read().unwrap();
                            subs.keys().cloned().collect()
                        };
                        for signature in &signatures {
                            Self::check_signature(
                                signature,
                                current_slot,
                                &bank_forks,
                                signature_subscriptions.clone(),
                                &notifier,
                            );
                        }
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
    use solana_budget_program;
    use solana_ledger::genesis_utils::{create_genesis_config, GenesisConfigInfo};
    use solana_sdk::{
        signature::{Keypair, Signer},
        system_transaction,
    };
    use std::{fmt::Debug, sync::mpsc::channel, time::Instant};
    use tokio::{prelude::FutureExt, runtime::Runtime, timer::Delay};

    pub(crate) fn robust_poll_or_panic<T: Debug + Send + 'static>(
        receiver: futures::sync::mpsc::Receiver<T>,
    ) -> T {
        let (inner_sender, inner_receiver) = channel();
        let mut rt = Runtime::new().unwrap();
        rt.spawn(futures::lazy(|| {
            let recv_timeout = receiver
                .into_future()
                .timeout(Duration::from_millis(RECEIVE_DELAY_MILLIS))
                .map(move |result| match result {
                    (Some(value), _) => inner_sender.send(value).expect("send error"),
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
    fn test_check_account_subscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));
        let alice = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &alice,
            blockhash,
            1,
            16,
            &solana_budget_program::id(),
        );
        bank_forks
            .write()
            .unwrap()
            .get(0)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();

        let (subscriber, _id_receiver, transport_receiver) =
            Subscriber::new_test("accountNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let subscriptions = RpcSubscriptions::new(&exit);
        subscriptions.add_account_subscription(&alice.pubkey(), None, &sub_id, &sink);

        assert!(subscriptions
            .account_subscriptions
            .read()
            .unwrap()
            .contains_key(&alice.pubkey()));

        subscriptions.notify_subscribers(0, &bank_forks);
        let response = robust_poll_or_panic(transport_receiver);
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "context": { "slot": 0 },
                   "value": {
                       "data": "1111111111111111",
                       "executable": false,
                       "lamports": 1,
                       "owner": "Budget1111111111111111111111111111111111111",
                       "rentEpoch": 1,
                    },
               },
               "subscription": 0,
           }
        });
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);

        subscriptions.remove_account_subscription(&sub_id);
        assert!(!subscriptions
            .account_subscriptions
            .read()
            .unwrap()
            .contains_key(&alice.pubkey()));
    }

    #[test]
    fn test_check_program_subscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));
        let alice = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &alice,
            blockhash,
            1,
            16,
            &solana_budget_program::id(),
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
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let subscriptions = RpcSubscriptions::new(&exit);
        subscriptions.add_program_subscription(&solana_budget_program::id(), None, &sub_id, &sink);

        assert!(subscriptions
            .program_subscriptions
            .read()
            .unwrap()
            .contains_key(&solana_budget_program::id()));

        subscriptions.notify_subscribers(0, &bank_forks);
        let response = robust_poll_or_panic(transport_receiver);
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
                          "owner": "Budget1111111111111111111111111111111111111",
                          "rentEpoch": 1,
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
            .program_subscriptions
            .read()
            .unwrap()
            .contains_key(&solana_budget_program::id()));
    }

    #[test]
    fn test_check_signature_subscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));
        let alice = Keypair::new();
        let tx = system_transaction::transfer(&mint_keypair, &alice.pubkey(), 20, blockhash);
        let signature = tx.signatures[0];
        let unprocessed_tx =
            system_transaction::transfer(&mint_keypair, &alice.pubkey(), 10, blockhash);
        let not_ready_signature = unprocessed_tx.signatures[0];
        bank_forks
            .write()
            .unwrap()
            .get(0)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();

        let (subscriber, _id_receiver, transport_receiver) =
            Subscriber::new_test("signatureNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let remaining_sub_id = SubscriptionId::Number(1 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let subscriptions = RpcSubscriptions::new(&exit);
        subscriptions.add_signature_subscription(&signature, None, &sub_id, &sink.clone());
        subscriptions.add_signature_subscription(
            &not_ready_signature,
            None,
            &remaining_sub_id,
            &sink.clone(),
        );

        {
            let sig_subs = subscriptions.signature_subscriptions.read().unwrap();
            assert!(sig_subs.contains_key(&signature));
            assert!(sig_subs.contains_key(&not_ready_signature));
        }

        subscriptions.notify_subscribers(0, &bank_forks);
        let response = robust_poll_or_panic(transport_receiver);
        let expected_res: Option<transaction::Result<()>> = Some(Ok(()));
        let expected = json!({
            "jsonrpc": "2.0",
            "method": "signatureNotification",
            "params": {
                "result": {
                    "context": { "slot": 0 },
                    "value": expected_res,
                },
                "subscription": 0,
            }
        });
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);

        // Subscription should be automatically removed after notification
        assert!(!subscriptions
            .signature_subscriptions
            .read()
            .unwrap()
            .contains_key(&signature));

        // Unprocessed signature subscription should not be removed
        assert!(subscriptions
            .signature_subscriptions
            .read()
            .unwrap()
            .contains_key(&not_ready_signature));
    }

    #[test]
    fn test_check_slot_subscribe() {
        let (subscriber, _id_receiver, transport_receiver) =
            Subscriber::new_test("slotNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let subscriptions = RpcSubscriptions::new(&exit);
        subscriptions.add_slot_subscription(&sub_id, &sink);

        assert!(subscriptions
            .slot_subscriptions
            .read()
            .unwrap()
            .contains_key(&sub_id));

        subscriptions.notify_slot(0, 0, 0);
        let response = robust_poll_or_panic(transport_receiver);
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
            .slot_subscriptions
            .read()
            .unwrap()
            .contains_key(&sub_id));
    }

    #[test]
    fn test_add_and_remove_subscription() {
        let (subscriber, _id_receiver, _transport_receiver) = Subscriber::new_test("notification");
        let sink = subscriber
            .assign_id(SubscriptionId::String("test".to_string()))
            .unwrap();
        let mut subscriptions: HashMap<u64, HashMap<SubscriptionId, (Sink<()>, Confirmations)>> =
            HashMap::new();

        let num_keys = 5;
        let mut next_id: u64 = 0;
        for _ in 0..num_keys {
            let key = next_id;
            let sub_id = SubscriptionId::Number(next_id);
            add_subscription(&mut subscriptions, &key, None, &sub_id, &sink.clone());
            next_id += 1;
        }

        // Add another subscription to the "0" key
        let sub_id = SubscriptionId::Number(next_id);
        add_subscription(&mut subscriptions, &0, None, &sub_id, &sink.clone());

        assert_eq!(subscriptions.len(), num_keys);
        assert_eq!(subscriptions.get(&0).unwrap().len(), 2);
        assert_eq!(subscriptions.get(&1).unwrap().len(), 1);

        assert_eq!(
            remove_subscription(&mut subscriptions, &SubscriptionId::Number(0)),
            true
        );
        assert_eq!(subscriptions.len(), num_keys);
        assert_eq!(subscriptions.get(&0).unwrap().len(), 1);
        assert_eq!(
            remove_subscription(&mut subscriptions, &SubscriptionId::Number(0)),
            false
        );

        assert_eq!(
            remove_subscription(&mut subscriptions, &SubscriptionId::Number(next_id)),
            true
        );
        assert_eq!(subscriptions.len(), num_keys - 1);
        assert!(subscriptions.get(&0).is_none());
    }
}

//! The `pubsub` module implements a threaded subscription service on client RPC request

use crate::bank::{Bank, BankSubscriptions};
use crate::jsonrpc_core::futures::Future;
use crate::jsonrpc_core::*;
use crate::jsonrpc_macros::pubsub;
use crate::jsonrpc_macros::pubsub::Sink;
use crate::jsonrpc_pubsub::{PubSubHandler, Session, SubscriptionId};
use crate::jsonrpc_ws_server::{RequestContext, Sender, ServerBuilder};
use crate::rpc::RpcSignatureStatus;
use crate::service::Service;
use crate::status_deque::Status;
use bs58;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::collections::HashMap;
use std::mem;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{atomic, Arc, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

pub enum ClientState {
    Uninitialized,
    Init(Sender),
}

pub struct PubSubService {
    thread_hdl: JoinHandle<()>,
    exit: Arc<AtomicBool>,
    rpc_bank: Arc<RwLock<RpcPubSubBank>>,
    subscription: Arc<RpcSubscriptions>,
}

impl Service for PubSubService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

impl PubSubService {
    pub fn new(bank: &Arc<Bank>, pubsub_addr: SocketAddr) -> Self {
        info!("rpc_pubsub bound to {:?}", pubsub_addr);
        let rpc_bank = Arc::new(RwLock::new(RpcPubSubBank::new(bank.clone())));
        let rpc = RpcSolPubSubImpl::new(rpc_bank.clone());
        let subscription = rpc.subscription.clone();
        bank.update_subscriptions(&subscription);
        let exit = Arc::new(AtomicBool::new(false));
        let exit_ = exit.clone();
        let thread_hdl = Builder::new()
            .name("solana-pubsub".to_string())
            .spawn(move || {
                let mut io = PubSubHandler::default();
                io.extend_with(rpc.to_delegate());

                let server = ServerBuilder::with_meta_extractor(io, |context: &RequestContext| {
                        info!("New pubsub connection");
                        let session = Arc::new(Session::new(context.sender().clone()));
                        session.on_drop(|| {
                            info!("Pubsub connection dropped");
                        });
                        session
                })
                .start(&pubsub_addr);

                if let Err(e) = server {
                    warn!("Pubsub service unavailable error: {:?}. \nAlso, check that port {} is not already in use by another application", e, pubsub_addr.port());
                    return;
                }
                while !exit_.load(Ordering::Relaxed) {
                    sleep(Duration::from_millis(100));
                }
                server.unwrap().close();
            })
            .unwrap();
        PubSubService {
            thread_hdl,
            exit,
            rpc_bank,
            subscription,
        }
    }

    pub fn set_bank(&self, bank: &Arc<Bank>) {
        self.rpc_bank.write().unwrap().bank = bank.clone();
        bank.update_subscriptions(&self.subscription);
    }

    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn close(self) -> thread::Result<()> {
        self.exit();
        self.join()
    }
}

build_rpc_trait! {
    pub trait RpcSolPubSub {
        type Metadata;

        #[pubsub(name = "accountNotification")] {
            // Get notification every time account userdata is changed
            // Accepts pubkey parameter as base-58 encoded string
            #[rpc(name = "accountSubscribe")]
            fn account_subscribe(&self, Self::Metadata, pubsub::Subscriber<Account>, String);

            // Unsubscribe from account notification subscription.
            #[rpc(name = "accountUnsubscribe")]
            fn account_unsubscribe(&self, SubscriptionId) -> Result<bool>;
        }
        #[pubsub(name = "signatureNotification")] {
            // Get notification when signature is verified
            // Accepts signature parameter as base-58 encoded string
            #[rpc(name = "signatureSubscribe")]
            fn signature_subscribe(&self, Self::Metadata, pubsub::Subscriber<RpcSignatureStatus>, String);

            // Unsubscribe from signature notification subscription.
            #[rpc(name = "signatureUnsubscribe")]
            fn signature_unsubscribe(&self, SubscriptionId) -> Result<bool>;
        }
    }
}

struct RpcPubSubBank {
    bank: Arc<Bank>,
}

impl RpcPubSubBank {
    pub fn new(bank: Arc<Bank>) -> Self {
        RpcPubSubBank { bank }
    }
}

pub struct RpcSubscriptions {
    account_subscriptions: RwLock<HashMap<Pubkey, HashMap<SubscriptionId, Sink<Account>>>>,
    signature_subscriptions:
        RwLock<HashMap<Signature, HashMap<SubscriptionId, Sink<RpcSignatureStatus>>>>,
}

impl Default for RpcSubscriptions {
    fn default() -> Self {
        RpcSubscriptions {
            account_subscriptions: Default::default(),
            signature_subscriptions: Default::default(),
        }
    }
}

impl BankSubscriptions for RpcSubscriptions {
    fn check_account(&self, pubkey: &Pubkey, account: &Account) {
        let subscriptions = self.account_subscriptions.read().unwrap();
        if let Some(hashmap) = subscriptions.get(pubkey) {
            for (_bank_sub_id, sink) in hashmap.iter() {
                sink.notify(Ok(account.clone())).wait().unwrap();
            }
        }
    }

    fn check_signature(&self, signature: &Signature, status: RpcSignatureStatus) {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        if let Some(hashmap) = subscriptions.get(signature) {
            for (_bank_sub_id, sink) in hashmap.iter() {
                sink.notify(Ok(status)).wait().unwrap();
            }
        }
        subscriptions.remove(&signature);
    }
}

impl RpcSubscriptions {
    pub fn add_account_subscription(
        &self,
        pubkey: &Pubkey,
        sub_id: &SubscriptionId,
        sink: &Sink<Account>,
    ) {
        let mut subscriptions = self.account_subscriptions.write().unwrap();
        if let Some(current_hashmap) = subscriptions.get_mut(pubkey) {
            current_hashmap.insert(sub_id.clone(), sink.clone());
            return;
        }
        let mut hashmap = HashMap::new();
        hashmap.insert(sub_id.clone(), sink.clone());
        subscriptions.insert(*pubkey, hashmap);
    }

    pub fn remove_account_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.account_subscriptions.write().unwrap();
        let mut found = false;
        subscriptions.retain(|_, v| {
            v.retain(|k, _| {
                if *k == *id {
                    found = true;
                }
                !found
            });
            !v.is_empty()
        });
        found
    }

    pub fn add_signature_subscription(
        &self,
        signature: &Signature,
        sub_id: &SubscriptionId,
        sink: &Sink<RpcSignatureStatus>,
    ) {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        if let Some(current_hashmap) = subscriptions.get_mut(signature) {
            current_hashmap.insert(sub_id.clone(), sink.clone());
            return;
        }
        let mut hashmap = HashMap::new();
        hashmap.insert(sub_id.clone(), sink.clone());
        subscriptions.insert(*signature, hashmap);
    }

    pub fn remove_signature_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        let mut found = false;
        subscriptions.retain(|_, v| {
            v.retain(|k, _| {
                if *k == *id {
                    found = true;
                }
                !found
            });
            !v.is_empty()
        });
        found
    }
}

struct RpcSolPubSubImpl {
    uid: Arc<atomic::AtomicUsize>,
    bank: Arc<RwLock<RpcPubSubBank>>,
    subscription: Arc<RpcSubscriptions>,
}

impl RpcSolPubSubImpl {
    fn new(bank: Arc<RwLock<RpcPubSubBank>>) -> Self {
        RpcSolPubSubImpl {
            uid: Default::default(),
            bank,
            subscription: Arc::new(Default::default()),
        }
    }
}

impl RpcSolPubSub for RpcSolPubSubImpl {
    type Metadata = Arc<Session>;

    fn account_subscribe(
        &self,
        _meta: Self::Metadata,
        subscriber: pubsub::Subscriber<Account>,
        pubkey_str: String,
    ) {
        let pubkey_vec = bs58::decode(pubkey_str).into_vec().unwrap();
        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            subscriber
                .reject(Error {
                    code: ErrorCode::InvalidParams,
                    message: "Invalid Request: Invalid pubkey provided".into(),
                    data: None,
                })
                .unwrap();
            return;
        }
        let pubkey = Pubkey::new(&pubkey_vec);

        let id = self.uid.fetch_add(1, atomic::Ordering::SeqCst);
        let sub_id = SubscriptionId::Number(id as u64);
        info!("account_subscribe: account={:?} id={:?}", pubkey, sub_id);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();

        self.subscription
            .add_account_subscription(&pubkey, &sub_id, &sink)
    }

    fn account_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        info!("account_unsubscribe: id={:?}", id);
        if self.subscription.remove_account_subscription(&id) {
            Ok(true)
        } else {
            Err(Error {
                code: ErrorCode::InvalidParams,
                message: "Invalid Request: Subscription id does not exist".into(),
                data: None,
            })
        }
    }

    fn signature_subscribe(
        &self,
        _meta: Self::Metadata,
        subscriber: pubsub::Subscriber<RpcSignatureStatus>,
        signature_str: String,
    ) {
        info!("signature_subscribe");
        let signature_vec = bs58::decode(signature_str).into_vec().unwrap();
        if signature_vec.len() != mem::size_of::<Signature>() {
            subscriber
                .reject(Error {
                    code: ErrorCode::InvalidParams,
                    message: "Invalid Request: Invalid signature provided".into(),
                    data: None,
                })
                .unwrap();
            return;
        }
        let signature = Signature::new(&signature_vec);
        let id = self.uid.fetch_add(1, atomic::Ordering::SeqCst);
        let sub_id = SubscriptionId::Number(id as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();

        let status = self
            .bank
            .read()
            .unwrap()
            .bank
            .get_signature_status(&signature);
        if status.is_none() {
            self.subscription
                .add_signature_subscription(&signature, &sub_id, &sink);
            return;
        }

        match status.unwrap() {
            Status::Complete(Ok(_)) => {
                sink.notify(Ok(RpcSignatureStatus::Confirmed))
                    .wait()
                    .unwrap();
            }
            _ => self
                .subscription
                .add_signature_subscription(&signature, &sub_id, &sink),
        }
    }

    fn signature_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        info!("signature_unsubscribe");
        if self.subscription.remove_signature_subscription(&id) {
            Ok(true)
        } else {
            Err(Error {
                code: ErrorCode::InvalidParams,
                message: "Invalid Request: Subscription id does not exist".into(),
                data: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrpc_core::futures::sync::mpsc;
    use crate::jsonrpc_macros::pubsub::{Subscriber, SubscriptionId};
    use crate::mint::Mint;
    use solana_sdk::budget_program;
    use solana_sdk::budget_transaction::BudgetTransaction;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction::SystemTransaction;
    use solana_sdk::transaction::Transaction;
    use std::net::{IpAddr, Ipv4Addr};
    use tokio::prelude::{Async, Stream};

    #[test]
    fn test_pubsub_new() {
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let pubsub_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let pubsub_service = PubSubService::new(&Arc::new(bank), pubsub_addr);
        let thread = pubsub_service.thread_hdl.thread();
        assert_eq!(thread.name().unwrap(), "solana-pubsub");
    }

    #[test]
    #[ignore]
    fn test_signature_subscribe() {
        let alice = Mint::new(10_000);
        let bob = Keypair::new();
        let bob_pubkey = bob.pubkey();
        let bank = Bank::new(&alice);
        let arc_bank = Arc::new(bank);
        let last_id = arc_bank.last_id();

        let (sender, mut receiver) = mpsc::channel(1);
        let session = Arc::new(Session::new(sender));

        let mut io = PubSubHandler::default();
        let rpc_bank = Arc::new(RwLock::new(RpcPubSubBank::new(arc_bank.clone())));
        let rpc = RpcSolPubSubImpl::new(rpc_bank.clone());
        io.extend_with(rpc.to_delegate());

        // Test signature subscription
        let tx = Transaction::system_move(&alice.keypair(), bob_pubkey, 20, last_id, 0);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"signatureSubscribe","params":["{}"]}}"#,
            tx.signatures[0].to_string()
        );
        let res = io.handle_request_sync(&req, session.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","result":0,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Test bad parameter
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"signatureSubscribe","params":["a1b2c3"]}}"#
        );
        let res = io.handle_request_sync(&req, session.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32602,"message":"Invalid Request: Invalid signature provided"}},"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        arc_bank
            .process_transaction(&tx)
            .expect("process transaction");
        sleep(Duration::from_millis(200));

        // Test signature confirmation notification
        let string = receiver.poll();
        assert!(string.is_ok());
        if let Async::Ready(Some(response)) = string.unwrap() {
            let expected = format!(r#"{{"jsonrpc":"2.0","method":"signatureNotification","params":{{"result":"Confirmed","subscription":0}}}}"#);
            assert_eq!(expected, response);
        }

        // Test subscription id increment
        let tx = Transaction::system_move(&alice.keypair(), bob_pubkey, 10, last_id, 0);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"signatureSubscribe","params":["{}"]}}"#,
            tx.signatures[0].to_string()
        );
        let res = io.handle_request_sync(&req, session.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","result":1,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_signature_unsubscribe() {
        let alice = Mint::new(10_000);
        let bob_pubkey = Keypair::new().pubkey();
        let bank = Bank::new(&alice);
        let arc_bank = Arc::new(bank);
        let last_id = arc_bank.last_id();

        let (sender, _receiver) = mpsc::channel(1);
        let session = Arc::new(Session::new(sender));

        let mut io = PubSubHandler::default();
        let rpc_bank = Arc::new(RwLock::new(RpcPubSubBank::new(arc_bank.clone())));
        let rpc = RpcSolPubSubImpl::new(rpc_bank.clone());
        io.extend_with(rpc.to_delegate());

        let tx = Transaction::system_move(&alice.keypair(), bob_pubkey, 20, last_id, 0);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"signatureSubscribe","params":["{}"]}}"#,
            tx.signatures[0].to_string()
        );
        let _res = io.handle_request_sync(&req, session.clone());

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"signatureUnsubscribe","params":[0]}}"#);
        let res = io.handle_request_sync(&req, session.clone());

        let expected = format!(r#"{{"jsonrpc":"2.0","result":true,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Test bad parameter
        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"signatureUnsubscribe","params":[1]}}"#);
        let res = io.handle_request_sync(&req, session.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32602,"message":"Invalid Request: Subscription id does not exist"}},"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    #[ignore]
    fn test_account_subscribe() {
        let alice = Mint::new(10_000);
        let bob_pubkey = Keypair::new().pubkey();
        let witness = Keypair::new();
        let contract_funds = Keypair::new();
        let contract_state = Keypair::new();
        let budget_program_id = budget_program::id();
        let loader = Pubkey::default(); // TODO
        let executable = false; // TODO
        let bank = Bank::new(&alice);
        let arc_bank = Arc::new(bank);
        let last_id = arc_bank.last_id();

        let (sender, mut receiver) = mpsc::channel(1);
        let session = Arc::new(Session::new(sender));

        let mut io = PubSubHandler::default();
        let rpc_bank = Arc::new(RwLock::new(RpcPubSubBank::new(arc_bank.clone())));
        let rpc = RpcSolPubSubImpl::new(rpc_bank.clone());
        io.extend_with(rpc.to_delegate());

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"accountSubscribe","params":["{}"]}}"#,
            contract_state.pubkey().to_string()
        );

        let res = io.handle_request_sync(&req, session.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","result":0,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Test bad parameter
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"accountSubscribe","params":["a1b2c3"]}}"#
        );
        let res = io.handle_request_sync(&req, session.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32602,"message":"Invalid Request: Invalid pubkey provided"}},"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        let tx = Transaction::system_create(
            &alice.keypair(),
            contract_funds.pubkey(),
            last_id,
            50,
            0,
            budget_program_id,
            0,
        );
        arc_bank
            .process_transaction(&tx)
            .expect("process transaction");

        let tx = Transaction::system_create(
            &alice.keypair(),
            contract_state.pubkey(),
            last_id,
            1,
            196,
            budget_program_id,
            0,
        );

        arc_bank
            .process_transaction(&tx)
            .expect("process transaction");

        // Test signature confirmation notification #1
        let string = receiver.poll();
        assert!(string.is_ok());

        let expected_userdata = arc_bank
            .get_account(&contract_state.pubkey())
            .unwrap()
            .userdata;

        let expected = json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "owner": budget_program_id,
                   "tokens": 1,
                   "userdata": expected_userdata,
                   "executable": executable,
                   "loader": loader,

               },
               "subscription": 0,
           }
        });

        if let Async::Ready(Some(response)) = string.unwrap() {
            assert_eq!(serde_json::to_string(&expected).unwrap(), response);
        }

        let tx = Transaction::budget_new_when_signed(
            &contract_funds,
            bob_pubkey,
            contract_state.pubkey(),
            witness.pubkey(),
            None,
            50,
            last_id,
        );
        arc_bank
            .process_transaction(&tx)
            .expect("process transaction");
        sleep(Duration::from_millis(200));

        // Test signature confirmation notification #2
        let string = receiver.poll();
        assert!(string.is_ok());
        let expected_userdata = arc_bank
            .get_account(&contract_state.pubkey())
            .unwrap()
            .userdata;
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "owner": budget_program_id,
                   "tokens": 51,
                   "userdata": expected_userdata,
                    "executable": executable,
                   "loader": loader,
               },
               "subscription": 0,
           }
        });

        if let Async::Ready(Some(response)) = string.unwrap() {
            assert_eq!(serde_json::to_string(&expected).unwrap(), response);
        }

        let tx = Transaction::system_new(&alice.keypair(), witness.pubkey(), 1, last_id);
        arc_bank
            .process_transaction(&tx)
            .expect("process transaction");
        sleep(Duration::from_millis(200));
        let tx = Transaction::budget_new_signature(
            &witness,
            contract_state.pubkey(),
            bob_pubkey,
            last_id,
        );
        arc_bank
            .process_transaction(&tx)
            .expect("process transaction");
        sleep(Duration::from_millis(200));

        let expected_userdata = arc_bank
            .get_account(&contract_state.pubkey())
            .unwrap()
            .userdata;
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "owner": budget_program_id,
                   "tokens": 1,
                   "userdata": expected_userdata,
                    "executable": executable,
                   "loader": loader,
               },
               "subscription": 0,
           }
        });
        let string = receiver.poll();
        assert!(string.is_ok());
        if let Async::Ready(Some(response)) = string.unwrap() {
            assert_eq!(serde_json::to_string(&expected).unwrap(), response);
        }
    }

    #[test]
    fn test_account_unsubscribe() {
        let alice = Mint::new(10_000);
        let bob_pubkey = Keypair::new().pubkey();
        let bank = Bank::new(&alice);
        let arc_bank = Arc::new(bank);

        let (sender, _receiver) = mpsc::channel(1);
        let session = Arc::new(Session::new(sender));

        let mut io = PubSubHandler::default();
        let rpc_bank = Arc::new(RwLock::new(RpcPubSubBank::new(arc_bank.clone())));
        let rpc = RpcSolPubSubImpl::new(rpc_bank.clone());

        io.extend_with(rpc.to_delegate());

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"accountSubscribe","params":["{}"]}}"#,
            bob_pubkey.to_string()
        );
        let _res = io.handle_request_sync(&req, session.clone());

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"accountUnsubscribe","params":[0]}}"#);
        let res = io.handle_request_sync(&req, session.clone());

        let expected = format!(r#"{{"jsonrpc":"2.0","result":true,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Test bad parameter
        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"accountUnsubscribe","params":[1]}}"#);
        let res = io.handle_request_sync(&req, session.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32602,"message":"Invalid Request: Subscription id does not exist"}},"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_check_account_subscribe() {
        let mint = Mint::new(100);
        let bank = Bank::new(&mint);
        let alice = Keypair::new();
        let last_id = bank.last_id();
        let tx = Transaction::system_create(
            &mint.keypair(),
            alice.pubkey(),
            last_id,
            1,
            16,
            budget_program::id(),
            0,
        );
        bank.process_transaction(&tx).unwrap();

        let (subscriber, _id_receiver, mut transport_receiver) =
            Subscriber::new_test("accountNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let subscriptions = RpcSubscriptions::default();
        subscriptions.add_account_subscription(&alice.pubkey(), &sub_id, &sink);

        assert!(subscriptions
            .account_subscriptions
            .write()
            .unwrap()
            .contains_key(&alice.pubkey()));

        let account = bank.get_account(&alice.pubkey()).unwrap();
        subscriptions.check_account(&alice.pubkey(), &account);
        let string = transport_receiver.poll();
        assert!(string.is_ok());
        if let Async::Ready(Some(response)) = string.unwrap() {
            let expected = format!(r#"{{"jsonrpc":"2.0","method":"accountNotification","params":{{"result":{{"executable":false,"loader":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"owner":[129,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"tokens":1,"userdata":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}},"subscription":0}}}}"#);
            assert_eq!(expected, response);
        }

        subscriptions.remove_account_subscription(&sub_id);
        assert!(!subscriptions
            .account_subscriptions
            .write()
            .unwrap()
            .contains_key(&alice.pubkey()));
    }
    #[test]
    fn test_check_signature_subscribe() {
        let mint = Mint::new(100);
        let bank = Bank::new(&mint);
        let alice = Keypair::new();
        let last_id = bank.last_id();
        let tx = Transaction::system_move(&mint.keypair(), alice.pubkey(), 20, last_id, 0);
        let signature = tx.signatures[0];
        bank.process_transaction(&tx).unwrap();

        let (subscriber, _id_receiver, mut transport_receiver) =
            Subscriber::new_test("signatureNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let subscriptions = RpcSubscriptions::default();
        subscriptions.add_signature_subscription(&signature, &sub_id, &sink);

        assert!(subscriptions
            .signature_subscriptions
            .write()
            .unwrap()
            .contains_key(&signature));

        subscriptions.check_signature(&signature, RpcSignatureStatus::Confirmed);
        let string = transport_receiver.poll();
        assert!(string.is_ok());
        if let Async::Ready(Some(response)) = string.unwrap() {
            let expected = format!(r#"{{"jsonrpc":"2.0","method":"signatureNotification","params":{{"result":"Confirmed","subscription":0}}}}"#);
            assert_eq!(expected, response);
        }

        subscriptions.remove_signature_subscription(&sub_id);
        assert!(!subscriptions
            .signature_subscriptions
            .write()
            .unwrap()
            .contains_key(&signature));
    }
}

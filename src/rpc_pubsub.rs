//! The `pubsub` module implements a threaded subscription service on client RPC request

use bank::Bank;
use bs58;
use jsonrpc_core::futures::Future;
use jsonrpc_core::*;
use jsonrpc_macros::pubsub;
use jsonrpc_pubsub::{PubSubHandler, Session, SubscriptionId};
use jsonrpc_ws_server::{RequestContext, Sender, ServerBuilder};
use rpc::{JsonRpcRequestProcessor, RpcSignatureStatus};
use service::Service;
use signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
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
}

impl Service for PubSubService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

impl PubSubService {
    pub fn new(bank: &Arc<Bank>, pubsub_addr: SocketAddr) -> Self {
        let rpc = RpcSolPubSubImpl::new(JsonRpcRequestProcessor::new(bank.clone()), bank.clone());
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
                        session.on_drop(Box::new(|| {
                            info!("Pubsub connection dropped");
                        }));
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
                ()
            })
            .unwrap();
        PubSubService { thread_hdl, exit }
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

struct RpcSolPubSubImpl {
    uid: Arc<atomic::AtomicUsize>,
    request_processor: JsonRpcRequestProcessor,
    bank: Arc<Bank>,
    account_subscriptions: Arc<RwLock<HashMap<SubscriptionId, (Pubkey, Pubkey)>>>,
    signature_subscriptions: Arc<RwLock<HashMap<SubscriptionId, (Pubkey, Signature)>>>,
}

impl RpcSolPubSubImpl {
    fn new(request_processor: JsonRpcRequestProcessor, bank: Arc<Bank>) -> Self {
        RpcSolPubSubImpl {
            uid: Default::default(),
            request_processor,
            bank,
            account_subscriptions: Default::default(),
            signature_subscriptions: Default::default(),
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
                }).unwrap();
            return;
        }
        let pubkey = Pubkey::new(&pubkey_vec);

        let id = self.uid.fetch_add(1, atomic::Ordering::SeqCst);
        let sub_id = SubscriptionId::Number(id as u64);
        info!("account_subscribe: account={:?} id={:?}", pubkey, sub_id);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let bank_sub_id = Keypair::new().pubkey();
        self.account_subscriptions
            .write()
            .unwrap()
            .insert(sub_id.clone(), (bank_sub_id, pubkey));

        self.bank
            .add_account_subscription(bank_sub_id, pubkey, sink);
    }

    fn account_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        info!("account_unsubscribe: id={:?}", id);
        if let Some((bank_sub_id, pubkey)) = self.account_subscriptions.write().unwrap().remove(&id)
        {
            self.bank.remove_account_subscription(&bank_sub_id, &pubkey);
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
                }).unwrap();
            return;
        }
        let signature = Signature::new(&signature_vec);

        let id = self.uid.fetch_add(1, atomic::Ordering::SeqCst);
        let sub_id = SubscriptionId::Number(id as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let bank_sub_id = Keypair::new().pubkey();
        self.signature_subscriptions
            .write()
            .unwrap()
            .insert(sub_id.clone(), (bank_sub_id, signature));

        match self.request_processor.get_signature_status(signature) {
            Ok(_) => {
                sink.notify(Ok(RpcSignatureStatus::Confirmed))
                    .wait()
                    .unwrap();
                self.signature_subscriptions
                    .write()
                    .unwrap()
                    .remove(&sub_id);
            }
            Err(_) => {
                self.bank
                    .add_signature_subscription(bank_sub_id, signature, sink);
            }
        }
    }

    fn signature_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        info!("signature_unsubscribe");
        if let Some((bank_sub_id, signature)) =
            self.signature_subscriptions.write().unwrap().remove(&id)
        {
            self.bank
                .remove_signature_subscription(&bank_sub_id, &signature);
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
    use budget_program;
    use budget_transaction::BudgetTransaction;
    use jsonrpc_core::futures::sync::mpsc;
    use mint::Mint;
    use signature::{Keypair, KeypairUtil};
    use solana_sdk::transaction::Transaction;
    use std::net::{IpAddr, Ipv4Addr};
    use system_transaction::SystemTransaction;
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
        let rpc = RpcSolPubSubImpl::new(
            JsonRpcRequestProcessor::new(arc_bank.clone()),
            arc_bank.clone(),
        );
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
        let rpc = RpcSolPubSubImpl::new(
            JsonRpcRequestProcessor::new(arc_bank.clone()),
            arc_bank.clone(),
        );
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
        let rpc = RpcSolPubSubImpl::new(
            JsonRpcRequestProcessor::new(arc_bank.clone()),
            arc_bank.clone(),
        );
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
        let rpc = RpcSolPubSubImpl::new(
            JsonRpcRequestProcessor::new(arc_bank.clone()),
            arc_bank.clone(),
        );

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
}

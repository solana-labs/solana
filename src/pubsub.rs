//! The `pubsub` module implements a threaded subscription service on client RPC request

use bank::Bank;
use bs58;
use jsonrpc_core::futures::Future;
use jsonrpc_core::*;
use jsonrpc_macros::pubsub;
use jsonrpc_pubsub::{PubSubHandler, PubSubMetadata, Session, SubscriptionId};
use jsonrpc_ws_server::ws;
use jsonrpc_ws_server::{RequestContext, Sender, ServerBuilder};
use rpc::{JsonRpcRequestProcessor, RpcSignatureStatus};
use signature::Signature;
use solana_program_interface::account::Account;
use solana_program_interface::pubkey::Pubkey;
use std::collections::HashSet;
use std::mem;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{atomic, Arc, Mutex, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

pub enum ClientState {
    Uninitialized,
    Init(Sender),
}

#[derive(Serialize)]
pub struct SubscriptionResponse {
    pub port: u16,
    pub path: String,
}

pub struct PubSubService {
    _thread_hdl: JoinHandle<()>,
}

impl PubSubService {
    pub fn new(
        bank: &Arc<Bank>,
        pubsub_addr: SocketAddr,
        path: Pubkey,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let request_processor = JsonRpcRequestProcessor::new(bank.clone());
        let status = Arc::new(Mutex::new(ClientState::Uninitialized));
        let client_status = status.clone();
        let _thread_hdl = Builder::new()
            .name("solana-pubsub".to_string())
            .spawn(move || {
                let mut io = PubSubHandler::default();
            	let rpc = RpcSolPubSubImpl::default();
            	io.extend_with(rpc.to_delegate());

            	let server = ServerBuilder::with_meta_extractor(io, move |context: &RequestContext|
                        {
                            *client_status.lock().unwrap() = ClientState::Init(context.out.clone());
                            Meta {
                                request_processor: request_processor.clone(),
                                session: Arc::new(Session::new(context.sender().clone())),
                        }
                    })
                    .request_middleware(move |req: &ws::Request|
                        if req.resource() != format!("/{}", path.to_string()) {
            				Some(ws::Response::new(403, "Client path incorrect or not provided"))
            			} else {
            				None
            			})
            		.start(&pubsub_addr);

                if server.is_err() {
                    warn!("Pubsub service unavailable: unable to bind to port {}. \nMake sure this port is not already in use by another application", pubsub_addr.port());
                    return;
                }
                loop {
                    if exit.load(Ordering::Relaxed) {
                        server.unwrap().close();
                        break;
                    }
                    if let ClientState::Init(ref mut sender) = *status.lock().unwrap() {
                        if sender.check_active().is_err() {
                            server.unwrap().close();
                            break;
                        }
                    }
                    sleep(Duration::from_millis(100));
                }
                ()
            })
            .unwrap();
        PubSubService { _thread_hdl }
    }
}

#[derive(Clone)]
pub struct Meta {
    pub request_processor: JsonRpcRequestProcessor,
    pub session: Arc<Session>,
}
impl Metadata for Meta {}
impl PubSubMetadata for Meta {
    fn session(&self) -> Option<Arc<Session>> {
        Some(self.session.clone())
    }
}

build_rpc_trait! {
    pub trait RpcSolPubSub {
        type Metadata;

        #[pubsub(name = "signature_notification")] {
            // Get notification when signature is verified
            // Accepts signature parameter as base-58 encoded string
            #[rpc(name = "signatureSubscribe", alias = ["sigSub", ])]
            fn signature_subscribe(&self, Self::Metadata, pubsub::Subscriber<RpcSignatureStatus>, String);

            // Unsubscribe from signature notification subscription.
            #[rpc(name = "signatureUnsubscribe", alias = ["sigUnsub", ])]
            fn signature_unsubscribe(&self, SubscriptionId) -> Result<bool>;
        }
        #[pubsub(name = "account_notification")] {
            // Get notification every time account userdata is changed
            // Accepts pubkey parameter as base-58 encoded string
            #[rpc(name = "accountSubscribe", alias = ["accountSub", ])]
            fn account_subscribe(&self, Self::Metadata, pubsub::Subscriber<Account>, String);

            // Unsubscribe from account notification subscription.
            #[rpc(name = "accountUnsubscribe", alias = ["accountUnsub", ])]
            fn account_unsubscribe(&self, SubscriptionId) -> Result<bool>;
        }
    }
}

#[derive(Default)]
struct RpcSolPubSubImpl {
    uid: atomic::AtomicUsize,
    subscriptions: Arc<RwLock<HashSet<SubscriptionId>>>,
}
impl RpcSolPubSub for RpcSolPubSubImpl {
    type Metadata = Meta;

    fn signature_subscribe(
        &self,
        meta: Self::Metadata,
        subscriber: pubsub::Subscriber<RpcSignatureStatus>,
        signature_str: String,
    ) {
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
        self.subscriptions.write().unwrap().insert(sub_id.clone());

        match meta.request_processor.get_signature_status(signature) {
            Ok(_) => {
                sink.notify(Ok(RpcSignatureStatus::Confirmed))
                    .wait()
                    .unwrap();
            }
            Err(_) => {
                let active_subscriptions = self.subscriptions.clone();
                thread::spawn(move || {
                    while active_subscriptions.read().unwrap().contains(&sub_id) {
                        if meta
                            .request_processor
                            .get_signature_status(signature)
                            .is_ok()
                        {
                            sink.notify(Ok(RpcSignatureStatus::Confirmed))
                                .wait()
                                .unwrap();
                            break;
                        }
                        sleep(Duration::from_millis(100));
                    }
                });
            }
        }
    }

    fn signature_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        let removed = self.subscriptions.write().unwrap().remove(&id);
        if removed {
            Ok(true)
        } else {
            Err(Error {
                code: ErrorCode::InvalidParams,
                message: "Invalid Request: Subscription id does not exist".into(),
                data: None,
            })
        }
    }
    fn account_subscribe(
        &self,
        meta: Self::Metadata,
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
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        self.subscriptions.write().unwrap().insert(sub_id.clone());

        let mut previous_userdata = None;
        let active_subscriptions = self.subscriptions.clone();
        thread::spawn(move || {
            while active_subscriptions.read().unwrap().contains(&sub_id) {
                if let Ok(account) = meta.request_processor.get_account_info(pubkey) {
                    if Some(account.clone().userdata) != previous_userdata {
                        sink.notify(Ok(account.clone())).wait().unwrap();
                        previous_userdata = Some(account.userdata);
                    }
                }
                sleep(Duration::from_millis(100));
            }
        });
    }

    fn account_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        let removed = self.subscriptions.write().unwrap().remove(&id);
        if removed {
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
    use budget_program::BudgetState;
    use budget_transaction::BudgetTransaction;
    use jsonrpc_core::futures::sync::mpsc;
    use mint::Mint;
    use signature::{Keypair, KeypairUtil};
    use std::net::{IpAddr, Ipv4Addr};
    use system_transaction::SystemTransaction;
    use tokio::prelude::{Async, Stream};
    use transaction::Transaction;

    #[test]
    fn test_pubsub_new() {
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let pubsub_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let pubkey = Keypair::new().pubkey();
        let exit = Arc::new(AtomicBool::new(false));
        let pubsub_service = PubSubService::new(&Arc::new(bank), pubsub_addr, pubkey, exit);
        let thread = pubsub_service._thread_hdl.thread();
        assert_eq!(thread.name().unwrap(), "solana-pubsub");
    }

    #[test]
    fn test_signature_subscribe() {
        let alice = Mint::new(10_000);
        let bob_pubkey = Keypair::new().pubkey();
        let bank = Bank::new(&alice);
        let arc_bank = Arc::new(bank);
        let last_id = arc_bank.last_id();

        let request_processor = JsonRpcRequestProcessor::new(arc_bank.clone());
        let (sender, mut receiver) = mpsc::channel(1);
        let session = Arc::new(Session::new(sender));

        let mut io = PubSubHandler::default();
        let rpc = RpcSolPubSubImpl::default();
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor,
            session,
        };

        // Test signature subscription
        let tx = Transaction::system_move(&alice.keypair(), bob_pubkey, 20, last_id, 0);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"signatureSubscribe","params":["{}"]}}"#,
            tx.signature.to_string()
        );
        let res = io.handle_request_sync(&req, meta.clone());
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
        let res = io.handle_request_sync(&req, meta.clone());
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
            let expected = format!(r#"{{"jsonrpc":"2.0","method":"signature_notification","params":{{"result":"Confirmed","subscription":0}}}}"#);
            assert_eq!(expected, response);
        }

        // Test subscription id increment
        let tx = Transaction::system_move(&alice.keypair(), bob_pubkey, 10, last_id, 0);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"signatureSubscribe","params":["{}"]}}"#,
            tx.signature.to_string()
        );
        let res = io.handle_request_sync(&req, meta.clone());
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

        let request_processor = JsonRpcRequestProcessor::new(arc_bank);
        let (sender, _receiver) = mpsc::channel(1);
        let session = Arc::new(Session::new(sender));

        let mut io = PubSubHandler::default();
        let rpc = RpcSolPubSubImpl::default();
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor,
            session: session.clone(),
        };

        let tx = Transaction::system_move(&alice.keypair(), bob_pubkey, 20, last_id, 0);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"signatureSubscribe","params":["{}"]}}"#,
            tx.signature.to_string()
        );
        let _res = io.handle_request_sync(&req, meta.clone());

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"signatureUnsubscribe","params":[0]}}"#);
        let res = io.handle_request_sync(&req, meta.clone());

        let expected = format!(r#"{{"jsonrpc":"2.0","result":true,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Test bad parameter
        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"signatureUnsubscribe","params":[1]}}"#);
        let res = io.handle_request_sync(&req, meta.clone());
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
        let budget_program_id = BudgetState::id();
        let bank = Bank::new(&alice);
        let arc_bank = Arc::new(bank);
        let last_id = arc_bank.last_id();

        let request_processor = JsonRpcRequestProcessor::new(arc_bank.clone());
        let (sender, mut receiver) = mpsc::channel(1);
        let session = Arc::new(Session::new(sender));

        let mut io = PubSubHandler::default();
        let rpc = RpcSolPubSubImpl::default();
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor,
            session,
        };

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"accountSubscribe","params":["{}"]}}"#,
            contract_state.pubkey().to_string()
        );

        let res = io.handle_request_sync(&req, meta.clone());
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
        let res = io.handle_request_sync(&req, meta.clone());
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

        // Test signature confirmation notification
        let string = receiver.poll();
        assert!(string.is_ok());
        let expected_userdata = arc_bank
            .get_account(&contract_state.pubkey())
            .unwrap()
            .userdata;
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "account_notification",
           "params": {
               "result": {
                   "program_id": budget_program_id,
                   "tokens": 51,
                   "userdata": expected_userdata
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
           "method": "account_notification",
           "params": {
               "result": {
                   "program_id": budget_program_id,
                   "tokens": 1,
                   "userdata": expected_userdata
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

        let request_processor = JsonRpcRequestProcessor::new(arc_bank);
        let (sender, _receiver) = mpsc::channel(1);
        let session = Arc::new(Session::new(sender));

        let mut io = PubSubHandler::default();
        let rpc = RpcSolPubSubImpl::default();
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor,
            session: session.clone(),
        };

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"accountSubscribe","params":["{}"]}}"#,
            bob_pubkey.to_string()
        );
        let _res = io.handle_request_sync(&req, meta.clone());

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"accountUnsubscribe","params":[0]}}"#);
        let res = io.handle_request_sync(&req, meta.clone());

        let expected = format!(r#"{{"jsonrpc":"2.0","result":true,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Test bad parameter
        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"accountUnsubscribe","params":[1]}}"#);
        let res = io.handle_request_sync(&req, meta.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32602,"message":"Invalid Request: Subscription id does not exist"}},"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }
}

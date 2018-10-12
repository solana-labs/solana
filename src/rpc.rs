//! The `rpc` module implements the Solana RPC interface.

use bank::{Bank, BankError};
use bincode::deserialize;
use bs58;
use cluster_info::{ClusterInfo, FULLNODE_PORT_RANGE};
use drone::DRONE_PORT;
use jsonrpc_core::*;
use jsonrpc_http_server::*;
use jsonrpc_macros::pubsub::Sink;
use netutil::find_available_port_in_range;
use rpc_pubsub::{PubSubService, SubscriptionResponse};
use service::Service;
use signature::{Keypair, KeypairUtil, Signature};
use solana_program_interface::account::Account;
use solana_program_interface::pubkey::Pubkey;
use std::mem;
use std::net::{SocketAddr, UdpSocket};
use std::result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;
use transaction::Transaction;
use wallet::request_airdrop;

pub const RPC_PORT: u16 = 8899;

pub struct JsonRpcService {
    thread_hdl: JoinHandle<()>,
}

impl JsonRpcService {
    pub fn new(
        bank: &Arc<Bank>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        rpc_addr: SocketAddr,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let request_processor = JsonRpcRequestProcessor::new(bank.clone());
        let info = cluster_info.clone();
        let exit_pubsub = exit.clone();
        let thread_hdl = Builder::new()
            .name("solana-jsonrpc".to_string())
            .spawn(move || {
                let mut io = MetaIoHandler::default();
                let rpc = RpcSolImpl;
                io.extend_with(rpc.to_delegate());

                let server =
                    ServerBuilder::with_meta_extractor(io, move |_req: &hyper::Request<hyper::Body>| Meta {
                        request_processor: request_processor.clone(),
                        cluster_info: info.clone(),
                        rpc_addr,
                        exit: exit_pubsub.clone(),
                    }).threads(4)
                        .cors(DomainsValidation::AllowOnly(vec![
                            AccessControlAllowOrigin::Any,
                        ]))
                        .start_http(&rpc_addr);
                if server.is_err() {
                    warn!("JSON RPC service unavailable: unable to bind to RPC port {}. \nMake sure this port is not already in use by another application", rpc_addr.port());
                    return;
                }
                loop {
                    if exit.load(Ordering::Relaxed) {
                        server.unwrap().close();
                        break;
                    }
                    sleep(Duration::from_millis(100));
                }
                ()
            })
            .unwrap();
        JsonRpcService { thread_hdl }
    }
}

impl Service for JsonRpcService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

#[derive(Clone)]
pub struct Meta {
    pub request_processor: JsonRpcRequestProcessor,
    pub cluster_info: Arc<RwLock<ClusterInfo>>,
    pub rpc_addr: SocketAddr,
    pub exit: Arc<AtomicBool>,
}
impl Metadata for Meta {}

#[derive(Copy, Clone, PartialEq, Serialize, Debug)]
pub enum RpcSignatureStatus {
    Confirmed,
    SignatureNotFound,
    ProgramRuntimeError,
    GenericFailure,
}

build_rpc_trait! {
    pub trait RpcSol {
        type Metadata;

        #[rpc(meta, name = "confirmTransaction")]
        fn confirm_transaction(&self, Self::Metadata, String) -> Result<bool>;

        #[rpc(meta, name = "getAccountInfo")]
        fn get_account_info(&self, Self::Metadata, String) -> Result<Account>;

        #[rpc(meta, name = "getBalance")]
        fn get_balance(&self, Self::Metadata, String) -> Result<i64>;

        #[rpc(meta, name = "getFinality")]
        fn get_finality(&self, Self::Metadata) -> Result<usize>;

        #[rpc(meta, name = "getLastId")]
        fn get_last_id(&self, Self::Metadata) -> Result<String>;

        #[rpc(meta, name = "getSignatureStatus")]
        fn get_signature_status(&self, Self::Metadata, String) -> Result<RpcSignatureStatus>;

        #[rpc(meta, name = "getTransactionCount")]
        fn get_transaction_count(&self, Self::Metadata) -> Result<u64>;

        #[rpc(meta, name= "requestAirdrop")]
        fn request_airdrop(&self, Self::Metadata, String, u64) -> Result<String>;

        #[rpc(meta, name = "sendTransaction")]
        fn send_transaction(&self, Self::Metadata, Vec<u8>) -> Result<String>;

        #[rpc(meta, name = "startSubscriptionChannel")]
        fn start_subscription_channel(&self, Self::Metadata) -> Result<SubscriptionResponse>;
    }
}

pub struct RpcSolImpl;
impl RpcSol for RpcSolImpl {
    type Metadata = Meta;

    fn confirm_transaction(&self, meta: Self::Metadata, id: String) -> Result<bool> {
        self.get_signature_status(meta, id)
            .map(|status| status == RpcSignatureStatus::Confirmed)
    }

    fn get_account_info(&self, meta: Self::Metadata, id: String) -> Result<Account> {
        let pubkey_vec = bs58::decode(id)
            .into_vec()
            .map_err(|_| Error::invalid_request())?;
        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            return Err(Error::invalid_request());
        }
        let pubkey = Pubkey::new(&pubkey_vec);
        meta.request_processor.get_account_info(pubkey)
    }
    fn get_balance(&self, meta: Self::Metadata, id: String) -> Result<i64> {
        let pubkey_vec = bs58::decode(id)
            .into_vec()
            .map_err(|_| Error::invalid_request())?;
        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            return Err(Error::invalid_request());
        }
        let pubkey = Pubkey::new(&pubkey_vec);
        meta.request_processor.get_balance(pubkey)
    }
    fn get_finality(&self, meta: Self::Metadata) -> Result<usize> {
        meta.request_processor.get_finality()
    }
    fn get_last_id(&self, meta: Self::Metadata) -> Result<String> {
        meta.request_processor.get_last_id()
    }
    fn get_signature_status(&self, meta: Self::Metadata, id: String) -> Result<RpcSignatureStatus> {
        let signature_vec = bs58::decode(id)
            .into_vec()
            .map_err(|_| Error::invalid_request())?;
        if signature_vec.len() != mem::size_of::<Signature>() {
            return Err(Error::invalid_request());
        }
        let signature = Signature::new(&signature_vec);
        Ok(
            match meta.request_processor.get_signature_status(signature) {
                Ok(_) => RpcSignatureStatus::Confirmed,
                Err(BankError::ProgramRuntimeError(_)) => RpcSignatureStatus::ProgramRuntimeError,
                Err(BankError::SignatureNotFound) => RpcSignatureStatus::SignatureNotFound,
                Err(err) => {
                    trace!("mapping {:?} to GenericFailure", err);
                    RpcSignatureStatus::GenericFailure
                }
            },
        )
    }
    fn get_transaction_count(&self, meta: Self::Metadata) -> Result<u64> {
        meta.request_processor.get_transaction_count()
    }
    fn request_airdrop(&self, meta: Self::Metadata, id: String, tokens: u64) -> Result<String> {
        let mut drone_addr = get_leader_addr(&meta.cluster_info)?;
        drone_addr.set_port(DRONE_PORT);
        let pubkey_vec = bs58::decode(id)
            .into_vec()
            .map_err(|_| Error::invalid_request())?;
        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            return Err(Error::invalid_request());
        }
        let pubkey = Pubkey::new(&pubkey_vec);
        let signature =
            request_airdrop(&drone_addr, &pubkey, tokens).map_err(|_| Error::internal_error())?;
        let now = Instant::now();
        let mut signature_status;
        loop {
            signature_status = meta.request_processor.get_signature_status(signature);

            if signature_status.is_ok() {
                return Ok(bs58::encode(signature).into_string());
            } else if now.elapsed().as_secs() > 5 {
                return Err(Error::internal_error());
            }
            sleep(Duration::from_millis(100));
        }
    }
    fn send_transaction(&self, meta: Self::Metadata, data: Vec<u8>) -> Result<String> {
        let transactions_addr = get_leader_addr(&meta.cluster_info)?;
        let tx: Transaction = deserialize(&data).map_err(|err| {
            debug!("send_transaction: deserialize error: {:?}", err);
            Error::invalid_request()
        })?;
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        transactions_socket
            .send_to(&data, transactions_addr)
            .map_err(|err| {
                debug!("send_transaction: send_to error: {:?}", err);
                Error::internal_error()
            })?;
        Ok(bs58::encode(tx.signature).into_string())
    }
    fn start_subscription_channel(&self, meta: Self::Metadata) -> Result<SubscriptionResponse> {
        let port: u16 = find_available_port_in_range(FULLNODE_PORT_RANGE).map_err(|_| Error {
            code: ErrorCode::InternalError,
            message: "No available port in range".into(),
            data: None,
        })?;
        let mut pubsub_addr = meta.rpc_addr;
        pubsub_addr.set_port(port);
        let pubkey = Keypair::new().pubkey();
        let _pubsub_service =
            PubSubService::new(&meta.request_processor.bank, pubsub_addr, pubkey, meta.exit);
        Ok(SubscriptionResponse {
            port,
            path: pubkey.to_string(),
        })
    }
}
#[derive(Clone)]
pub struct JsonRpcRequestProcessor {
    bank: Arc<Bank>,
}
impl JsonRpcRequestProcessor {
    /// Create a new request processor that wraps the given Bank.
    pub fn new(bank: Arc<Bank>) -> Self {
        JsonRpcRequestProcessor { bank }
    }

    /// Process JSON-RPC request items sent via JSON-RPC.
    pub fn get_account_info(&self, pubkey: Pubkey) -> Result<Account> {
        self.bank
            .get_account(&pubkey)
            .ok_or_else(Error::invalid_request)
    }
    fn get_balance(&self, pubkey: Pubkey) -> Result<i64> {
        let val = self.bank.get_balance(&pubkey);
        Ok(val)
    }
    fn get_finality(&self) -> Result<usize> {
        Ok(self.bank.finality())
    }
    fn get_last_id(&self) -> Result<String> {
        let id = self.bank.last_id();
        Ok(bs58::encode(id).into_string())
    }
    pub fn get_signature_status(&self, signature: Signature) -> result::Result<(), BankError> {
        self.bank.get_signature_status(&signature)
    }
    fn get_transaction_count(&self) -> Result<u64> {
        Ok(self.bank.transaction_count() as u64)
    }
    pub fn add_account_subscription(
        &self,
        bank_sub_id: Pubkey,
        pubkey: Pubkey,
        sink: Sink<Account>,
    ) {
        self.bank
            .add_account_subscription(bank_sub_id, pubkey, sink);
    }
    pub fn remove_account_subscription(&self, bank_sub_id: &Pubkey, pubkey: &Pubkey) {
        self.bank.remove_account_subscription(bank_sub_id, pubkey);
    }
    pub fn add_signature_subscription(
        &self,
        bank_sub_id: Pubkey,
        signature: Signature,
        sink: Sink<RpcSignatureStatus>,
    ) {
        self.bank
            .add_signature_subscription(bank_sub_id, signature, sink);
    }
    pub fn remove_signature_subscription(&self, bank_sub_id: &Pubkey, signature: &Signature) {
        self.bank
            .remove_signature_subscription(bank_sub_id, signature);
    }
}

fn get_leader_addr(cluster_info: &Arc<RwLock<ClusterInfo>>) -> Result<SocketAddr> {
    if let Some(leader_data) = cluster_info.read().unwrap().leader_data() {
        Ok(leader_data.contact_info.tpu)
    } else {
        Err(Error {
            code: ErrorCode::InternalError,
            message: "No leader detected".into(),
            data: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bank::Bank;
    use cluster_info::NodeInfo;
    use jsonrpc_core::Response;
    use mint::Mint;
    use signature::{Keypair, KeypairUtil};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use system_transaction::SystemTransaction;
    use transaction::Transaction;

    #[test]
    fn test_rpc_request() {
        let alice = Mint::new(10_000);
        let bob_pubkey = Keypair::new().pubkey();
        let bank = Bank::new(&alice);

        let last_id = bank.last_id();
        let tx = Transaction::system_move(&alice.keypair(), bob_pubkey, 20, last_id, 0);
        bank.process_transaction(&tx).expect("process transaction");

        let request_processor = JsonRpcRequestProcessor::new(Arc::new(bank));
        let cluster_info = Arc::new(RwLock::new(
            ClusterInfo::new(NodeInfo::new_unspecified()).unwrap(),
        ));
        let rpc_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let exit = Arc::new(AtomicBool::new(false));

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor,
            cluster_info,
            rpc_addr,
            exit,
        };

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getBalance","params":["{}"]}}"#,
            bob_pubkey
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","result":20,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getTransactionCount"}}"#);
        let res = io.handle_request_sync(&req, meta.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","result":1,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getAccountInfo","params":["{}"]}}"#,
            bob_pubkey
        );

        let res = io.handle_request_sync(&req, meta.clone());
        let expected = r#"{
            "jsonrpc":"2.0",
            "result":{
                "program_id": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
                "tokens": 20,
                "userdata": []
            },
            "id":1}
        "#;
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }
    #[test]
    fn test_rpc_request_bad_parameter_type() {
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"confirmTransaction","params":[1234567890]}"#;
        let meta = Meta {
            request_processor: JsonRpcRequestProcessor::new(Arc::new(bank)),
            cluster_info: Arc::new(RwLock::new(
                ClusterInfo::new(NodeInfo::new_unspecified()).unwrap(),
            )),
            rpc_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            exit: Arc::new(AtomicBool::new(false)),
        };

        let res = io.handle_request_sync(req, meta);
        let expected = r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid params: invalid type: integer `1234567890`, expected a string."},"id":1}"#;
        let expected: Response =
            serde_json::from_str(expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }
    #[test]
    fn test_rpc_request_bad_signature() {
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let req =
            r#"{"jsonrpc":"2.0","id":1,"method":"confirmTransaction","params":["a1b2c3d4e5"]}"#;
        let meta = Meta {
            request_processor: JsonRpcRequestProcessor::new(Arc::new(bank)),
            cluster_info: Arc::new(RwLock::new(
                ClusterInfo::new(NodeInfo::new_unspecified()).unwrap(),
            )),
            rpc_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            exit: Arc::new(AtomicBool::new(false)),
        };

        let res = io.handle_request_sync(req, meta);
        let expected =
            r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Invalid request"},"id":1}"#;
        let expected: Response =
            serde_json::from_str(expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }
    #[test]
    fn test_rpc_get_leader_addr() {
        let cluster_info = Arc::new(RwLock::new(
            ClusterInfo::new(NodeInfo::new_unspecified()).unwrap(),
        ));
        assert_eq!(
            get_leader_addr(&cluster_info),
            Err(Error {
                code: ErrorCode::InternalError,
                message: "No leader detected".into(),
                data: None,
            })
        );
        let leader = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        cluster_info.write().unwrap().insert(&leader);
        cluster_info.write().unwrap().set_leader(leader.id);
        assert_eq!(
            get_leader_addr(&cluster_info),
            Ok(socketaddr!("127.0.0.1:1234"))
        );
    }
}

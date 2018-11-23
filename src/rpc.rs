//! The `rpc` module implements the Solana RPC interface.

use bank::{Bank, BankError};
use bincode::{deserialize, serialize};
use bs58;
use cluster_info::ClusterInfo;
use jsonrpc_core::*;
use jsonrpc_http_server::*;
use packet::PACKET_DATA_SIZE;
use service::Service;
use signature::Signature;
use solana_drone::drone::{request_airdrop_transaction, DRONE_PORT};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use std::mem;
use std::net::{SocketAddr, UdpSocket};
use std::result;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;
use std::time::Instant;
use transaction::Transaction;

pub const RPC_PORT: u16 = 8899;

pub struct JsonRpcService {
    thread_hdl: JoinHandle<()>,
    exit: Arc<AtomicBool>,
}

impl JsonRpcService {
    pub fn new(
        bank: &Arc<Bank>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        rpc_addr: SocketAddr,
    ) -> Self {
        let exit = Arc::new(AtomicBool::new(false));
        let request_processor = JsonRpcRequestProcessor::new(bank.clone());
        let info = cluster_info.clone();
        let exit_pubsub = exit.clone();
        let exit_ = exit.clone();
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
                while !exit_.load(Ordering::Relaxed) {
                    sleep(Duration::from_millis(100));
                }
                server.unwrap().close();
                ()
            })
            .unwrap();
        JsonRpcService { thread_hdl, exit }
    }

    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn close(self) -> thread::Result<()> {
        self.exit();
        self.join()
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
    AccountInUse,
    Confirmed,
    GenericFailure,
    ProgramRuntimeError,
    SignatureNotFound,
}
impl FromStr for RpcSignatureStatus {
    type Err = Error;

    fn from_str(s: &str) -> Result<RpcSignatureStatus> {
        match s {
            "AccountInUse" => Ok(RpcSignatureStatus::AccountInUse),
            "Confirmed" => Ok(RpcSignatureStatus::Confirmed),
            "GenericFailure" => Ok(RpcSignatureStatus::GenericFailure),
            "ProgramRuntimeError" => Ok(RpcSignatureStatus::ProgramRuntimeError),
            "SignatureNotFound" => Ok(RpcSignatureStatus::SignatureNotFound),
            _ => Err(Error::parse_error()),
        }
    }
}

build_rpc_trait! {
    pub trait RpcSol {
        type Metadata;

        #[rpc(meta, name = "confirmTransaction")]
        fn confirm_transaction(&self, Self::Metadata, String) -> Result<bool>;

        #[rpc(meta, name = "getAccountInfo")]
        fn get_account_info(&self, Self::Metadata, String) -> Result<Account>;

        #[rpc(meta, name = "getBalance")]
        fn get_balance(&self, Self::Metadata, String) -> Result<u64>;

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
    }
}

pub struct RpcSolImpl;
impl RpcSol for RpcSolImpl {
    type Metadata = Meta;

    fn confirm_transaction(&self, meta: Self::Metadata, id: String) -> Result<bool> {
        info!("confirm_transaction rpc request received: {:?}", id);
        self.get_signature_status(meta, id)
            .map(|status| status == RpcSignatureStatus::Confirmed)
    }

    fn get_account_info(&self, meta: Self::Metadata, id: String) -> Result<Account> {
        info!("get_account_info rpc request received: {:?}", id);
        let pubkey = verify_pubkey(id)?;
        meta.request_processor.get_account_info(pubkey)
    }
    fn get_balance(&self, meta: Self::Metadata, id: String) -> Result<u64> {
        info!("get_balance rpc request received: {:?}", id);
        let pubkey = verify_pubkey(id)?;
        meta.request_processor.get_balance(pubkey)
    }
    fn get_finality(&self, meta: Self::Metadata) -> Result<usize> {
        info!("get_finality rpc request received");
        meta.request_processor.get_finality()
    }
    fn get_last_id(&self, meta: Self::Metadata) -> Result<String> {
        info!("get_last_id rpc request received");
        meta.request_processor.get_last_id()
    }
    fn get_signature_status(&self, meta: Self::Metadata, id: String) -> Result<RpcSignatureStatus> {
        info!("get_signature_status rpc request received: {:?}", id);
        let signature = verify_signature(&id)?;
        Ok(
            match meta.request_processor.get_signature_status(signature) {
                Ok(_) => RpcSignatureStatus::Confirmed,
                Err(BankError::AccountInUse) => RpcSignatureStatus::AccountInUse,
                Err(BankError::ProgramError(_, _)) => RpcSignatureStatus::ProgramRuntimeError,
                // Report SignatureReserved as SignatureNotFound as SignatureReserved is
                // transitory while the bank processes the associated transaction.
                Err(BankError::SignatureReserved) => RpcSignatureStatus::SignatureNotFound,
                Err(BankError::SignatureNotFound) => RpcSignatureStatus::SignatureNotFound,
                Err(err) => {
                    trace!("mapping {:?} to GenericFailure", err);
                    RpcSignatureStatus::GenericFailure
                }
            },
        )
    }
    fn get_transaction_count(&self, meta: Self::Metadata) -> Result<u64> {
        info!("get_transaction_count rpc request received");
        meta.request_processor.get_transaction_count()
    }
    fn request_airdrop(&self, meta: Self::Metadata, id: String, tokens: u64) -> Result<String> {
        trace!("request_airdrop id={} tokens={}", id, tokens);
        let pubkey = verify_pubkey(id)?;

        let mut drone_addr = get_leader_addr(&meta.cluster_info)?;
        drone_addr.set_port(DRONE_PORT);
        let last_id = meta.request_processor.bank.last_id();
        let transaction = request_airdrop_transaction(&drone_addr, &pubkey, tokens, last_id)
            .map_err(|err| {
                info!("request_airdrop_transaction failed: {:?}", err);
                Error::internal_error()
            })?;;

        let data = serialize(&transaction).map_err(|err| {
            info!("request_airdrop: serialize error: {:?}", err);
            Error::internal_error()
        })?;

        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let transactions_addr = get_leader_addr(&meta.cluster_info)?;
        transactions_socket
            .send_to(&data, transactions_addr)
            .map_err(|err| {
                info!("request_airdrop: send_to error: {:?}", err);
                Error::internal_error()
            })?;

        let signature = transaction.signatures[0];
        let now = Instant::now();
        let mut signature_status;
        loop {
            signature_status = meta.request_processor.get_signature_status(signature);

            if signature_status.is_ok() {
                info!("airdrop signature ok");
                return Ok(bs58::encode(signature).into_string());
            } else if now.elapsed().as_secs() > 5 {
                info!("airdrop signature timeout");
                return Err(Error::internal_error());
            }
            sleep(Duration::from_millis(100));
        }
    }
    fn send_transaction(&self, meta: Self::Metadata, data: Vec<u8>) -> Result<String> {
        let tx: Transaction = deserialize(&data).map_err(|err| {
            info!("send_transaction: deserialize error: {:?}", err);
            Error::invalid_request()
        })?;
        if data.len() >= PACKET_DATA_SIZE {
            info!(
                "send_transaction: transaction too large: {} bytes (max: {} bytes)",
                data.len(),
                PACKET_DATA_SIZE
            );
            return Err(Error::invalid_request());
        }
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let transactions_addr = get_leader_addr(&meta.cluster_info)?;
        transactions_socket
            .send_to(&data, transactions_addr)
            .map_err(|err| {
                info!("send_transaction: send_to error: {:?}", err);
                Error::internal_error()
            })?;
        let signature = bs58::encode(tx.signatures[0]).into_string();
        trace!(
            "send_transaction: sent {} bytes, signature={}",
            data.len(),
            signature
        );
        Ok(signature)
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
    fn get_balance(&self, pubkey: Pubkey) -> Result<u64> {
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
}

fn get_leader_addr(cluster_info: &Arc<RwLock<ClusterInfo>>) -> Result<SocketAddr> {
    if let Some(leader_data) = cluster_info.read().unwrap().leader_data() {
        Ok(leader_data.tpu)
    } else {
        Err(Error {
            code: ErrorCode::InternalError,
            message: "No leader detected".into(),
            data: None,
        })
    }
}

fn verify_pubkey(input: String) -> Result<Pubkey> {
    let pubkey_vec = bs58::decode(input).into_vec().map_err(|err| {
        info!("verify_pubkey: invalid input: {:?}", err);
        Error::invalid_request()
    })?;
    if pubkey_vec.len() != mem::size_of::<Pubkey>() {
        info!(
            "verify_pubkey: invalid pubkey_vec length: {}",
            pubkey_vec.len()
        );
        Err(Error::invalid_request())
    } else {
        Ok(Pubkey::new(&pubkey_vec))
    }
}

fn verify_signature(input: &str) -> Result<Signature> {
    let signature_vec = bs58::decode(input).into_vec().map_err(|err| {
        info!("verify_signature: invalid input: {}: {:?}", input, err);
        Error::invalid_request()
    })?;
    if signature_vec.len() != mem::size_of::<Signature>() {
        info!(
            "verify_signature: invalid signature_vec length: {}",
            signature_vec.len()
        );
        Err(Error::invalid_request())
    } else {
        Ok(Signature::new(&signature_vec))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bank::Bank;
    use bincode::serialize;
    use cluster_info::{Node, NodeInfo};
    use fullnode::Fullnode;
    use jsonrpc_core::Response;
    use leader_scheduler::LeaderScheduler;
    use ledger::create_tmp_ledger_with_mint;
    use mint::Mint;
    use reqwest;
    use reqwest::header::CONTENT_TYPE;
    use signature::{Keypair, KeypairUtil};
    use solana_sdk::hash::{hash, Hash};
    use std::fs::remove_dir_all;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use system_transaction::SystemTransaction;
    use transaction::Transaction;

    fn start_rpc_handler_with_tx(pubkey: Pubkey) -> (MetaIoHandler<Meta>, Meta, Hash, Keypair) {
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);

        let last_id = bank.last_id();
        let tx = Transaction::system_move(&alice.keypair(), pubkey, 20, last_id, 0);
        bank.process_transaction(&tx).expect("process transaction");

        let request_processor = JsonRpcRequestProcessor::new(Arc::new(bank));
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(NodeInfo::default())));
        let leader = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        cluster_info.write().unwrap().insert_info(leader.clone());
        cluster_info.write().unwrap().set_leader(leader.id);
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
        (io, meta, last_id, alice.keypair())
    }

    #[test]
    #[ignore]
    fn test_rpc_new() {
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(NodeInfo::default())));
        let rpc_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 24680);
        let rpc_service = JsonRpcService::new(&Arc::new(bank), &cluster_info, rpc_addr);
        let thread = rpc_service.thread_hdl.thread();
        assert_eq!(thread.name().unwrap(), "solana-jsonrpc");

        let rpc_string = format!("http://{}", rpc_addr.to_string());
        let client = reqwest::Client::new();
        let request = json!({
           "jsonrpc": "2.0",
           "id": 1,
           "method": "getBalance",
           "params": [alice.pubkey().to_string()],
        });
        let mut response = client
            .post(&rpc_string)
            .header(CONTENT_TYPE, "application/json")
            .body(request.to_string())
            .send()
            .unwrap();
        let json: Value = serde_json::from_str(&response.text().unwrap()).unwrap();

        assert_eq!(10_000, json["result"].as_u64().unwrap());
    }

    #[test]
    fn test_rpc_request_processor_new() {
        let alice = Mint::new(10_000);
        let bob_pubkey = Keypair::new().pubkey();
        let bank = Bank::new(&alice);
        let arc_bank = Arc::new(bank);
        let request_processor = JsonRpcRequestProcessor::new(arc_bank.clone());
        thread::spawn(move || {
            let last_id = arc_bank.last_id();
            let tx = Transaction::system_move(&alice.keypair(), bob_pubkey, 20, last_id, 0);
            arc_bank
                .process_transaction(&tx)
                .expect("process transaction");
        }).join()
        .unwrap();
        assert_eq!(request_processor.get_transaction_count().unwrap(), 1);
    }

    #[test]
    fn test_rpc_get_balance() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, _last_id, _alice_keypair) = start_rpc_handler_with_tx(bob_pubkey);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getBalance","params":["{}"]}}"#,
            bob_pubkey
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(r#"{{"jsonrpc":"2.0","result":20,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_tx_count() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, _last_id, _alice_keypair) = start_rpc_handler_with_tx(bob_pubkey);

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getTransactionCount"}}"#);
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(r#"{{"jsonrpc":"2.0","result":1,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_account_info() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, _last_id, _alice_keypair) = start_rpc_handler_with_tx(bob_pubkey);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getAccountInfo","params":["{}"]}}"#,
            bob_pubkey
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = r#"{
            "jsonrpc":"2.0",
            "result":{
                "owner": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
                "tokens": 20,
                "userdata": [],
                "executable": false,
                "loader": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
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
    fn test_rpc_confirm_tx() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, last_id, alice_keypair) = start_rpc_handler_with_tx(bob_pubkey);
        let tx = Transaction::system_move(&alice_keypair, bob_pubkey, 20, last_id, 0);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"confirmTransaction","params":["{}"]}}"#,
            tx.signatures[0]
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(r#"{{"jsonrpc":"2.0","result":true,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_signature_status() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, last_id, alice_keypair) = start_rpc_handler_with_tx(bob_pubkey);
        let tx = Transaction::system_move(&alice_keypair, bob_pubkey, 20, last_id, 0);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getSignatureStatus","params":["{}"]}}"#,
            tx.signatures[0]
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","result":"Confirmed","id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Test getSignatureStatus request on unprocessed tx
        let tx = Transaction::system_move(&alice_keypair, bob_pubkey, 10, last_id, 0);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getSignatureStatus","params":["{}"]}}"#,
            tx.signatures[0]
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(r#"{{"jsonrpc":"2.0","result":"SignatureNotFound","id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_finality() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, _last_id, _alice_keypair) = start_rpc_handler_with_tx(bob_pubkey);

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getFinality"}}"#);
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(r#"{{"jsonrpc":"2.0","result":18446744073709551615,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_last_id() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, last_id, _alice_keypair) = start_rpc_handler_with_tx(bob_pubkey);

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getLastId"}}"#);
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(r#"{{"jsonrpc":"2.0","result":"{}","id":1}}"#, last_id);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_fail_request_airdrop() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, _last_id, _alice_keypair) = start_rpc_handler_with_tx(bob_pubkey);

        // Expect internal error because no leader is running
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"requestAirdrop","params":["{}", 50]}}"#,
            bob_pubkey
        );
        let res = io.handle_request_sync(&req, meta);
        let expected =
            r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"Internal error"},"id":1}"#;
        let expected: Response =
            serde_json::from_str(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_send_tx() {
        let leader_keypair = Arc::new(Keypair::new());
        let leader = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        let alice = Mint::new(10_000_000);
        let mut bank = Bank::new(&alice);
        let bob_pubkey = Keypair::new().pubkey();
        let leader_data = leader.info.clone();
        let ledger_path = create_tmp_ledger_with_mint("rpc_send_tx", &alice);

        let last_id = bank.last_id();
        let tx = Transaction::system_move(&alice.keypair(), bob_pubkey, 20, last_id, 0);
        let serial_tx = serialize(&tx).unwrap();

        let leader_scheduler = Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
            leader_data.id,
        )));
        bank.leader_scheduler = leader_scheduler;

        let vote_account_keypair = Arc::new(Keypair::new());
        let entry_height = alice.create_entries().len() as u64;
        let server = Fullnode::new_with_bank(
            leader_keypair,
            vote_account_keypair,
            bank,
            entry_height,
            &last_id,
            leader,
            None,
            &ledger_path,
            false,
            None,
        );
        sleep(Duration::from_millis(900));

        let client = reqwest::Client::new();
        let request = json!({
           "jsonrpc": "2.0",
           "id": 1,
           "method": "sendTransaction",
           "params": json!([serial_tx])
        });
        let rpc_addr = leader_data.rpc;
        let rpc_string = format!("http://{}", rpc_addr.to_string());
        let mut response = client
            .post(&rpc_string)
            .header(CONTENT_TYPE, "application/json")
            .body(request.to_string())
            .send()
            .unwrap();
        let json: Value = serde_json::from_str(&response.text().unwrap()).unwrap();
        let signature = &json["result"];

        sleep(Duration::from_millis(500));

        let client = reqwest::Client::new();
        let request = json!({
           "jsonrpc": "2.0",
           "id": 1,
           "method": "confirmTransaction",
           "params": [signature],
        });
        let mut response = client
            .post(&rpc_string)
            .header(CONTENT_TYPE, "application/json")
            .body(request.to_string())
            .send()
            .unwrap();
        let response_json_text = response.text().unwrap();
        let json: Value = serde_json::from_str(&response_json_text).unwrap();

        assert_eq!(true, json["result"]);

        server.close().unwrap();
        remove_dir_all(ledger_path).unwrap();
    }

    #[test]
    fn test_rpc_send_bad_tx() {
        let alice = Mint::new(10_000);
        let bank = Bank::new(&alice);

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor: JsonRpcRequestProcessor::new(Arc::new(bank)),
            cluster_info: Arc::new(RwLock::new(ClusterInfo::new(NodeInfo::default()))),
            rpc_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            exit: Arc::new(AtomicBool::new(false)),
        };

        let req =
            r#"{"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":[[0,0,0,0,0,0,0,0]]}"#;
        let res = io.handle_request_sync(req, meta.clone());
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
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(NodeInfo::default())));
        assert_eq!(
            get_leader_addr(&cluster_info),
            Err(Error {
                code: ErrorCode::InternalError,
                message: "No leader detected".into(),
                data: None,
            })
        );
        let leader = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        cluster_info.write().unwrap().insert_info(leader.clone());
        cluster_info.write().unwrap().set_leader(leader.id);
        assert_eq!(
            get_leader_addr(&cluster_info),
            Ok(socketaddr!("127.0.0.1:1234"))
        );
    }

    #[test]
    fn test_rpc_verify_pubkey() {
        let pubkey = Keypair::new().pubkey();
        assert_eq!(verify_pubkey(pubkey.to_string()).unwrap(), pubkey);
        let bad_pubkey = "a1b2c3d4";
        assert_eq!(
            verify_pubkey(bad_pubkey.to_string()),
            Err(Error::invalid_request())
        );
    }

    #[test]
    fn test_rpc_verify_signature() {
        let tx =
            Transaction::system_move(&Keypair::new(), Keypair::new().pubkey(), 20, hash(&[0]), 0);
        assert_eq!(
            verify_signature(&tx.signatures[0].to_string()).unwrap(),
            tx.signatures[0]
        );
        let bad_signature = "a1b2c3d4";
        assert_eq!(
            verify_signature(&bad_signature.to_string()),
            Err(Error::invalid_request())
        );
    }
}

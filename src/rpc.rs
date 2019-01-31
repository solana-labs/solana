//! The `rpc` module implements the Solana RPC interface.

use crate::bank::{self, Bank, BankError};
use crate::cluster_info::ClusterInfo;
use crate::jsonrpc_core::*;
use crate::jsonrpc_http_server::*;
use crate::packet::PACKET_DATA_SIZE;
use crate::service::Service;
use crate::storage_stage::StorageState;
use bincode::{deserialize, serialize};
use bs58;
use solana_drone::drone::request_airdrop_transaction;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::transaction::Transaction;
use std::mem;
use std::net::{SocketAddr, UdpSocket};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::{Duration, Instant};

pub const RPC_PORT: u16 = 8899;

pub struct JsonRpcService {
    thread_hdl: JoinHandle<()>,
    exit: Arc<AtomicBool>,
    request_processor: Arc<RwLock<JsonRpcRequestProcessor>>,
}

impl JsonRpcService {
    pub fn new(
        bank: &Arc<Bank>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        rpc_addr: SocketAddr,
        drone_addr: SocketAddr,
        storage_state: StorageState,
    ) -> Self {
        info!("rpc bound to {:?}", rpc_addr);
        let exit = Arc::new(AtomicBool::new(false));
        let request_processor = Arc::new(RwLock::new(JsonRpcRequestProcessor::new(
            bank.clone(),
            storage_state,
        )));
        request_processor.write().unwrap().bank = bank.clone();
        let request_processor_ = request_processor.clone();

        let info = cluster_info.clone();
        let exit_ = exit.clone();

        let thread_hdl = Builder::new()
            .name("solana-jsonrpc".to_string())
            .spawn(move || {
                let mut io = MetaIoHandler::default();
                let rpc = RpcSolImpl;
                io.extend_with(rpc.to_delegate());

                let server =
                    ServerBuilder::with_meta_extractor(io, move |_req: &hyper::Request<hyper::Body>| Meta {
                        request_processor: request_processor_.clone(),
                        cluster_info: info.clone(),
                        drone_addr,
                        rpc_addr,
                    }).threads(4)
                        .cors(DomainsValidation::AllowOnly(vec![
                            AccessControlAllowOrigin::Any,
                        ]))
                        .start_http(&rpc_addr);
                if let Err(e) = server {
                    warn!("JSON RPC service unavailable error: {:?}. \nAlso, check that port {} is not already in use by another application", e, rpc_addr.port());
                    return;
                }
                while !exit_.load(Ordering::Relaxed) {
                    sleep(Duration::from_millis(100));
                }
                server.unwrap().close();
            })
            .unwrap();
        Self {
            thread_hdl,
            exit,
            request_processor,
        }
    }

    pub fn set_bank(&mut self, bank: &Arc<Bank>) {
        self.request_processor.write().unwrap().bank = bank.clone();
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
    pub request_processor: Arc<RwLock<JsonRpcRequestProcessor>>,
    pub cluster_info: Arc<RwLock<ClusterInfo>>,
    pub rpc_addr: SocketAddr,
    pub drone_addr: SocketAddr,
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

        #[rpc(meta, name = "getConfirmationTime")]
        fn get_confirmation_time(&self, Self::Metadata) -> Result<usize>;

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

        #[rpc(meta, name = "getStorageMiningLastId")]
        fn get_storage_mining_last_id(&self, Self::Metadata) -> Result<String>;

        #[rpc(meta, name = "getStorageMiningEntryHeight")]
        fn get_storage_mining_entry_height(&self, Self::Metadata) -> Result<u64>;

        #[rpc(meta, name = "getStoragePubkeysForEntryHeight")]
        fn get_storage_pubkeys_for_entry_height(&self, Self::Metadata, u64) -> Result<Vec<Pubkey>>;
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
        meta.request_processor
            .read()
            .unwrap()
            .get_account_info(pubkey)
    }
    fn get_balance(&self, meta: Self::Metadata, id: String) -> Result<u64> {
        info!("get_balance rpc request received: {:?}", id);
        let pubkey = verify_pubkey(id)?;
        meta.request_processor.read().unwrap().get_balance(pubkey)
    }
    fn get_confirmation_time(&self, meta: Self::Metadata) -> Result<usize> {
        info!("get_confirmation_time rpc request received");
        meta.request_processor
            .read()
            .unwrap()
            .get_confirmation_time()
    }
    fn get_last_id(&self, meta: Self::Metadata) -> Result<String> {
        info!("get_last_id rpc request received");
        meta.request_processor.read().unwrap().get_last_id()
    }
    fn get_signature_status(&self, meta: Self::Metadata, id: String) -> Result<RpcSignatureStatus> {
        info!("get_signature_status rpc request received: {:?}", id);
        let signature = verify_signature(&id)?;
        let res = meta
            .request_processor
            .read()
            .unwrap()
            .get_signature_status(signature);

        let status = {
            if res.is_none() {
                RpcSignatureStatus::SignatureNotFound
            } else {
                match res.unwrap() {
                    Ok(_) => RpcSignatureStatus::Confirmed,
                    Err(BankError::AccountInUse) => RpcSignatureStatus::AccountInUse,
                    Err(BankError::ProgramError(_, _)) => RpcSignatureStatus::ProgramRuntimeError,
                    Err(err) => {
                        trace!("mapping {:?} to GenericFailure", err);
                        RpcSignatureStatus::GenericFailure
                    }
                }
            }
        };
        info!("get_signature_status rpc request status: {:?}", status);
        Ok(status)
    }
    fn get_transaction_count(&self, meta: Self::Metadata) -> Result<u64> {
        info!("get_transaction_count rpc request received");
        meta.request_processor
            .read()
            .unwrap()
            .get_transaction_count()
    }
    fn request_airdrop(&self, meta: Self::Metadata, id: String, tokens: u64) -> Result<String> {
        trace!("request_airdrop id={} tokens={}", id, tokens);
        let pubkey = verify_pubkey(id)?;

        let last_id = meta.request_processor.read().unwrap().bank.last_id();
        let transaction = request_airdrop_transaction(&meta.drone_addr, &pubkey, tokens, last_id)
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
            signature_status = meta
                .request_processor
                .read()
                .unwrap()
                .get_signature_status(signature);

            if signature_status == Some(Ok(())) {
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
        trace!("send_transaction: leader is {:?}", &transactions_addr);
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
    fn get_storage_mining_last_id(&self, meta: Self::Metadata) -> Result<String> {
        meta.request_processor
            .read()
            .unwrap()
            .get_storage_mining_last_id()
    }
    fn get_storage_mining_entry_height(&self, meta: Self::Metadata) -> Result<u64> {
        meta.request_processor
            .read()
            .unwrap()
            .get_storage_mining_entry_height()
    }
    fn get_storage_pubkeys_for_entry_height(
        &self,
        meta: Self::Metadata,
        entry_height: u64,
    ) -> Result<Vec<Pubkey>> {
        meta.request_processor
            .read()
            .unwrap()
            .get_storage_pubkeys_for_entry_height(entry_height)
    }
}
#[derive(Clone)]
pub struct JsonRpcRequestProcessor {
    bank: Arc<Bank>,
    storage_state: StorageState,
}
impl JsonRpcRequestProcessor {
    /// Create a new request processor that wraps the given Bank.
    pub fn new(bank: Arc<Bank>, storage_state: StorageState) -> Self {
        JsonRpcRequestProcessor {
            bank,
            storage_state,
        }
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
    fn get_confirmation_time(&self) -> Result<usize> {
        Ok(self.bank.confirmation_time())
    }
    fn get_last_id(&self) -> Result<String> {
        let id = self.bank.last_id();
        Ok(bs58::encode(id).into_string())
    }
    pub fn get_signature_status(&self, signature: Signature) -> Option<bank::Result<()>> {
        self.bank.get_signature_status(&signature)
    }
    fn get_transaction_count(&self) -> Result<u64> {
        Ok(self.bank.transaction_count() as u64)
    }
    fn get_storage_mining_last_id(&self) -> Result<String> {
        let id = self.storage_state.get_last_id();
        Ok(bs58::encode(id).into_string())
    }
    fn get_storage_mining_entry_height(&self) -> Result<u64> {
        let entry_height = self.storage_state.get_entry_height();
        Ok(entry_height)
    }
    fn get_storage_pubkeys_for_entry_height(&self, entry_height: u64) -> Result<Vec<Pubkey>> {
        Ok(self
            .storage_state
            .get_pubkeys_for_entry_height(entry_height))
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
    use crate::bank::Bank;
    use crate::cluster_info::NodeInfo;
    use crate::genesis_block::GenesisBlock;
    use crate::jsonrpc_core::Response;
    use solana_sdk::hash::{hash, Hash};
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction::SystemTransaction;
    use solana_sdk::transaction::Transaction;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn start_rpc_handler_with_tx(pubkey: Pubkey) -> (MetaIoHandler<Meta>, Meta, Hash, Keypair) {
        let (genesis_block, alice) = GenesisBlock::new(10_000);
        let bank = Bank::new(&genesis_block);

        let last_id = bank.last_id();
        let tx = Transaction::system_move(&alice, pubkey, 20, last_id, 0);
        bank.process_transaction(&tx).expect("process transaction");

        let request_processor = Arc::new(RwLock::new(JsonRpcRequestProcessor::new(
            Arc::new(bank),
            StorageState::default(),
        )));
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(NodeInfo::default())));
        let leader = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));

        cluster_info.write().unwrap().insert_info(leader.clone());
        cluster_info.write().unwrap().set_leader(leader.id);
        let rpc_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let drone_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor,
            cluster_info,
            drone_addr,
            rpc_addr,
        };
        (io, meta, last_id, alice)
    }

    #[test]
    fn test_rpc_new() {
        let (genesis_block, alice) = GenesisBlock::new(10_000);
        let bank = Bank::new(&genesis_block);
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(NodeInfo::default())));
        let rpc_addr = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            solana_netutil::find_available_port_in_range((10000, 65535)).unwrap(),
        );
        let drone_addr = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            solana_netutil::find_available_port_in_range((10000, 65535)).unwrap(),
        );
        let rpc_service = JsonRpcService::new(
            &Arc::new(bank),
            &cluster_info,
            rpc_addr,
            drone_addr,
            StorageState::default(),
        );
        let thread = rpc_service.thread_hdl.thread();
        assert_eq!(thread.name().unwrap(), "solana-jsonrpc");

        assert_eq!(
            10_000,
            rpc_service
                .request_processor
                .read()
                .unwrap()
                .get_balance(alice.pubkey())
                .unwrap()
        );

        rpc_service.close().unwrap();
    }

    #[test]
    fn test_rpc_request_processor_new() {
        let (genesis_block, alice) = GenesisBlock::new(10_000);
        let bob_pubkey = Keypair::new().pubkey();
        let bank = Bank::new(&genesis_block);
        let arc_bank = Arc::new(bank);
        let request_processor =
            JsonRpcRequestProcessor::new(arc_bank.clone(), StorageState::default());
        thread::spawn(move || {
            let last_id = arc_bank.last_id();
            let tx = Transaction::system_move(&alice, bob_pubkey, 20, last_id, 0);
            arc_bank
                .process_transaction(&tx)
                .expect("process transaction");
        })
        .join()
        .unwrap();
        assert_eq!(request_processor.get_transaction_count().unwrap(), 1);
    }

    #[test]
    fn test_rpc_get_balance() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, _last_id, _alice) = start_rpc_handler_with_tx(bob_pubkey);

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
        let (io, meta, _last_id, _alice) = start_rpc_handler_with_tx(bob_pubkey);

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
        let (io, meta, _last_id, _alice) = start_rpc_handler_with_tx(bob_pubkey);

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
        let (io, meta, last_id, alice) = start_rpc_handler_with_tx(bob_pubkey);
        let tx = Transaction::system_move(&alice, bob_pubkey, 20, last_id, 0);

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
        let (io, meta, last_id, alice) = start_rpc_handler_with_tx(bob_pubkey);
        let tx = Transaction::system_move(&alice, bob_pubkey, 20, last_id, 0);

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
        let tx = Transaction::system_move(&alice, bob_pubkey, 10, last_id, 0);
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
    fn test_rpc_get_confirmation() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, _last_id, _alice) = start_rpc_handler_with_tx(bob_pubkey);

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getConfirmationTime"}}"#);
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
        let (io, meta, last_id, _alice) = start_rpc_handler_with_tx(bob_pubkey);

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
        let (io, meta, _last_id, _alice) = start_rpc_handler_with_tx(bob_pubkey);

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
    fn test_rpc_send_bad_tx() {
        let (genesis_block, _) = GenesisBlock::new(10_000);
        let bank = Bank::new(&genesis_block);

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor: Arc::new(RwLock::new(JsonRpcRequestProcessor::new(
                Arc::new(bank),
                StorageState::default(),
            ))),
            cluster_info: Arc::new(RwLock::new(ClusterInfo::new(NodeInfo::default()))),
            drone_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            rpc_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
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

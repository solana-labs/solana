//! The `rpc` module implements the Solana RPC interface.

use bank::Bank;
use bincode::deserialize;
use bs58;
use jsonrpc_core::*;
use jsonrpc_http_server::*;
use service::Service;
use signature::{Pubkey, Signature};
use std::mem;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
        bank: Arc<Bank>,
        transactions_addr: SocketAddr,
        drone_addr: SocketAddr,
        rpc_addr: SocketAddr,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let request_processor = JsonRpcRequestProcessor::new(bank);
        let thread_hdl = Builder::new()
            .name("solana-jsonrpc".to_string())
            .spawn(move || {
                let mut io = MetaIoHandler::default();
                let rpc = RpcSolImpl;
                io.extend_with(rpc.to_delegate());

                let server =
                    ServerBuilder::with_meta_extractor(io, move |_req: &hyper::Request| Meta {
                        request_processor: request_processor.clone(),
                        transactions_addr,
                        drone_addr,
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
                }
                ()
            })
            .unwrap();
        JsonRpcService { thread_hdl }
    }
}

impl Service for JsonRpcService {
    fn thread_hdls(self) -> Vec<JoinHandle<()>> {
        vec![self.thread_hdl]
    }

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

#[derive(Clone)]
pub struct Meta {
    pub request_processor: JsonRpcRequestProcessor,
    pub transactions_addr: SocketAddr,
    pub drone_addr: SocketAddr,
}
impl Metadata for Meta {}

build_rpc_trait! {
    pub trait RpcSol {
        type Metadata;

        #[rpc(meta, name = "confirmTransaction")]
        fn confirm_transaction(&self, Self::Metadata, String) -> Result<bool>;

        #[rpc(meta, name = "getBalance")]
        fn get_balance(&self, Self::Metadata, String) -> Result<i64>;

        #[rpc(meta, name = "getFinality")]
        fn get_finality(&self, Self::Metadata) -> Result<usize>;

        #[rpc(meta, name = "getLastId")]
        fn get_last_id(&self, Self::Metadata) -> Result<String>;

        #[rpc(meta, name = "getTransactionCount")]
        fn get_transaction_count(&self, Self::Metadata) -> Result<u64>;

        #[rpc(meta, name= "requestAirdrop")]
        fn request_airdrop(&self, Self::Metadata, String, u64) -> Result<bool>;

        #[rpc(meta, name = "sendTransaction")]
        fn send_transaction(&self, Self::Metadata, Vec<u8>) -> Result<String>;
    }
}

pub struct RpcSolImpl;
impl RpcSol for RpcSolImpl {
    type Metadata = Meta;

    fn confirm_transaction(&self, meta: Self::Metadata, id: String) -> Result<bool> {
        let signature_vec = bs58::decode(id)
            .into_vec()
            .map_err(|_| Error::invalid_request())?;
        if signature_vec.len() != mem::size_of::<Signature>() {
            return Err(Error::invalid_request());
        }
        let signature = Signature::new(&signature_vec);
        meta.request_processor.get_signature_status(signature)
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
    fn get_transaction_count(&self, meta: Self::Metadata) -> Result<u64> {
        meta.request_processor.get_transaction_count()
    }
    fn request_airdrop(&self, meta: Self::Metadata, id: String, tokens: u64) -> Result<bool> {
        let pubkey_vec = bs58::decode(id)
            .into_vec()
            .map_err(|_| Error::invalid_request())?;
        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            return Err(Error::invalid_request());
        }
        let pubkey = Pubkey::new(&pubkey_vec);
        let previous_balance = meta
            .request_processor
            .get_balance(pubkey)
            .map_err(|_| Error::internal_error())?;
        request_airdrop(&meta.drone_addr, &pubkey, tokens).map_err(|_| Error::internal_error())?;
        let now = Instant::now();
        let mut balance;
        loop {
            balance = meta
                .request_processor
                .get_balance(pubkey)
                .map_err(|_| Error::internal_error())?;
            if balance > previous_balance {
                return Ok(true);
            } else if now.elapsed().as_secs() > 5 {
                return Err(Error::internal_error());
            }
            sleep(Duration::from_millis(100));
        }
    }
    fn send_transaction(&self, meta: Self::Metadata, data: Vec<u8>) -> Result<String> {
        let tx: Transaction = deserialize(&data).map_err(|_| Error::invalid_request())?;
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        transactions_socket
            .send_to(&data, &meta.transactions_addr)
            .map_err(|_| Error::internal_error())?;
        Ok(bs58::encode(tx.signature).into_string())
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
    fn get_signature_status(&self, signature: Signature) -> Result<bool> {
        Ok(self.bank.has_signature(&signature))
    }
    fn get_transaction_count(&self) -> Result<u64> {
        Ok(self.bank.transaction_count() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bank::Bank;
    use jsonrpc_core::Response;
    use mint::Mint;
    use signature::{Keypair, KeypairUtil};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use transaction::Transaction;

    #[test]
    fn test_rpc_request() {
        let alice = Mint::new(10_000);
        let bob_pubkey = Keypair::new().pubkey();
        let bank = Bank::new(&alice);

        let last_id = bank.last_id();
        let tx = Transaction::new(&alice.keypair(), bob_pubkey, 20, last_id);
        bank.process_transaction(&tx).expect("process transaction");

        let request_processor = JsonRpcRequestProcessor::new(Arc::new(bank));
        let transactions_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let drone_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor,
            transactions_addr,
            drone_addr,
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
            transactions_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            drone_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
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
            transactions_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            drone_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
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
}

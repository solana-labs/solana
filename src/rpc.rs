//! The `rpc` module implements the Solana rpc interface.

use bank::Bank;
use bs58;
use jsonrpc_core::*;
use jsonrpc_http_server::*;
use request::{Request as JsonRpcRequest, Response};
use service::Service;
use signature::{Pubkey, Signature};
use std::mem;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread::{self, Builder, JoinHandle};

pub const RPC_PORT: u16 = 8899;

pub struct JsonRpcService {
    thread_hdl: JoinHandle<()>,
}

impl JsonRpcService {
    pub fn new(bank: Arc<Bank>, rpc_addr: SocketAddr) -> Self {
        let request_processor = JsonRpcRequestProcessor::new(bank);
        let thread_hdl = Builder::new()
            .name("solana-jsonrpc".to_string())
            .spawn(move || {
                let mut io = MetaIoHandler::default();
                let rpc = RpcSolImpl;
                io.extend_with(rpc.to_delegate());

                let server = ServerBuilder::new(io)
                    .meta_extractor(move |_req: &hyper::Request| Meta {
                        request_processor: Some(request_processor.clone()),
                    })
                    .threads(4)
                    .cors(DomainsValidation::AllowOnly(vec![
                        AccessControlAllowOrigin::Any,
                    ]))
                    .start_http(&rpc_addr)
                    .unwrap();
                server.wait();
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
    pub request_processor: Option<JsonRpcRequestProcessor>,
}
impl Metadata for Meta {}
impl Default for Meta {
    fn default() -> Self {
        Meta {
            request_processor: None,
        }
    }
}

build_rpc_trait! {
    pub trait RpcSol {
        type Metadata;

        #[rpc(meta, name = "solana_confirmTransaction")]
        fn confirm_transaction(&self, Self::Metadata, String) -> Result<bool>;

        #[rpc(meta, name = "solana_getBalance")]
        fn get_balance(&self, Self::Metadata, String) -> Result<(String, i64)>;

        #[rpc(meta, name = "solana_getFinality")]
        fn get_finality(&self, Self::Metadata) -> Result<usize>;

        #[rpc(meta, name = "solana_getLastId")]
        fn get_last_id(&self, Self::Metadata) -> Result<String>;

        #[rpc(meta, name = "solana_getTransactionCount")]
        fn get_transaction_count(&self, Self::Metadata) -> Result<u64>;

        // #[rpc(meta, name = "solana_sendTransaction")]
        // fn send_transaction(&self, Self::Metadata, String, i64) -> Result<String>;
    }
}

pub struct RpcSolImpl;
impl RpcSol for RpcSolImpl {
    type Metadata = Meta;

    fn confirm_transaction(&self, meta: Self::Metadata, id: String) -> Result<bool> {
        let signature_vec = bs58::decode(id)
            .into_vec()
            .expect("base58-encoded public key");

        if signature_vec.len() != mem::size_of::<Signature>() {
            Err(Error::invalid_request())
        } else {
            let signature = Signature::new(&signature_vec);
            let req = JsonRpcRequest::GetSignature { signature };
            let resp = meta.request_processor.unwrap().process_request(req);
            match resp {
                Some(Response::SignatureStatus { signature_status }) => Ok(signature_status),
                Some(_) => Err(Error {
                    code: ErrorCode::ServerError(-32002),
                    message: "Server error: bad response".to_string(),
                    data: None,
                }),
                None => Err(Error {
                    code: ErrorCode::ServerError(-32001),
                    message: "Server error: no node found".to_string(),
                    data: None,
                }),
            }
        }
    }
    fn get_balance(&self, meta: Self::Metadata, id: String) -> Result<(String, i64)> {
        let pubkey_vec = bs58::decode(id)
            .into_vec()
            .expect("base58-encoded public key");

        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            Err(Error::invalid_request())
        } else {
            let pubkey = Pubkey::new(&pubkey_vec);
            let req = JsonRpcRequest::GetBalance { key: pubkey };
            let resp = meta.request_processor.unwrap().process_request(req);
            match resp {
                Some(Response::Balance { key, val }) => Ok((bs58::encode(key).into_string(), val)),
                Some(_) => Err(Error {
                    code: ErrorCode::ServerError(-32002),
                    message: "Server error: bad response".to_string(),
                    data: None,
                }),
                None => Err(Error {
                    code: ErrorCode::ServerError(-32001),
                    message: "Server error: no node found".to_string(),
                    data: None,
                }),
            }
        }
    }
    fn get_finality(&self, meta: Self::Metadata) -> Result<usize> {
        let req = JsonRpcRequest::GetFinality;
        let resp = meta.request_processor.unwrap().process_request(req);
        match resp {
            Some(Response::Finality { time }) => Ok(time),
            Some(_) => Err(Error {
                code: ErrorCode::ServerError(-32002),
                message: "Server error: bad response".to_string(),
                data: None,
            }),
            None => Err(Error {
                code: ErrorCode::ServerError(-32001),
                message: "Server error: no node found".to_string(),
                data: None,
            }),
        }
    }
    fn get_last_id(&self, meta: Self::Metadata) -> Result<String> {
        let req = JsonRpcRequest::GetLastId;
        let resp = meta.request_processor.unwrap().process_request(req);
        match resp {
            Some(Response::LastId { id }) => Ok(bs58::encode(id).into_string()),
            Some(_) => Err(Error {
                code: ErrorCode::ServerError(-32002),
                message: "Server error: bad response".to_string(),
                data: None,
            }),
            None => Err(Error {
                code: ErrorCode::ServerError(-32001),
                message: "Server error: no node found".to_string(),
                data: None,
            }),
        }
    }
    fn get_transaction_count(&self, meta: Self::Metadata) -> Result<u64> {
        let req = JsonRpcRequest::GetTransactionCount;
        let resp = meta.request_processor.unwrap().process_request(req);
        match resp {
            Some(Response::TransactionCount { transaction_count }) => Ok(transaction_count),
            Some(_) => Err(Error {
                code: ErrorCode::ServerError(-32002),
                message: "Server error: bad response".to_string(),
                data: None,
            }),
            None => Err(Error {
                code: ErrorCode::ServerError(-32001),
                message: "Server error: no node found".to_string(),
                data: None,
            }),
        }
    }
    // fn send_transaction(&self, meta: Self::Metadata, to: String, tokens: i64) -> Result<String> {
    //     let client_keypair = read_keypair(&meta.keypair_location.unwrap()).unwrap();
    //     let mut client = mk_client(&meta.leader.unwrap());
    //     let last_id = client.get_last_id();
    //     let to_pubkey_vec = bs58::decode(to)
    //         .into_vec()
    //         .expect("base58-encoded public key");
    //
    //     if to_pubkey_vec.len() != mem::size_of::<Pubkey>() {
    //         Err(Error::invalid_request())
    //     } else {
    //         let to_pubkey = Pubkey::new(&to_pubkey_vec);
    //         let signature = client
    //             .transfer(tokens, &client_keypair, to_pubkey, &last_id)
    //             .unwrap();
    //         Ok(bs58::encode(signature).into_string())
    //     }
    // }
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

    /// Process Request items sent via JSON-RPC.
    fn process_request(&self, msg: JsonRpcRequest) -> Option<Response> {
        match msg {
            JsonRpcRequest::GetBalance { key } => {
                let val = self.bank.get_balance(&key);
                let rsp = Response::Balance { key, val };
                info!("Response::Balance {:?}", rsp);
                Some(rsp)
            }
            JsonRpcRequest::GetLastId => {
                let id = self.bank.last_id();
                let rsp = Response::LastId { id };
                info!("Response::LastId {:?}", rsp);
                Some(rsp)
            }
            JsonRpcRequest::GetTransactionCount => {
                let transaction_count = self.bank.transaction_count() as u64;
                let rsp = Response::TransactionCount { transaction_count };
                info!("Response::TransactionCount {:?}", rsp);
                Some(rsp)
            }
            JsonRpcRequest::GetSignature { signature } => {
                let signature_status = self.bank.has_signature(&signature);
                let rsp = Response::SignatureStatus { signature_status };
                info!("Response::Signature {:?}", rsp);
                Some(rsp)
            }
            JsonRpcRequest::GetFinality => {
                let time = self.bank.finality();
                let rsp = Response::Finality { time };
                info!("Response::Finality {:?}", rsp);
                Some(rsp)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bank::Bank;
    use jsonrpc_core::Response;
    use mint::Mint;
    use signature::{Keypair, KeypairUtil};
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

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor: Some(request_processor),
        };

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"solana_getBalance","params":["{}"]}}"#,
            bob_pubkey
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected = format!(
            r#"{{"jsonrpc":"2.0","result":["{}", 20],"id":1}}"#,
            bob_pubkey
        );
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");

        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"solana_getTransactionCount"}}"#);
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
        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"solana_getBalance","params":[1234567890]}"#;
        let meta = Meta {
            request_processor: None,
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
        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let req =
            r#"{"jsonrpc":"2.0","id":1,"method":"solana_confirmTransaction","params":["a1b2c3d4e5"]}"#;
        let meta = Meta {
            request_processor: None,
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

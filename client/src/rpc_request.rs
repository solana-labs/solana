use bs58;
use log::*;
use reqwest;
use reqwest::header::CONTENT_TYPE;
use serde_json::{json, Value};
use solana_sdk::account::Account;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::timing::{DEFAULT_TICKS_PER_SLOT, NUM_TICKS_PER_SECOND};
use std::io;
use std::net::SocketAddr;
use std::thread::sleep;
use std::time::{Duration, Instant};
use std::{error, fmt};

#[derive(Clone)]
pub struct RpcClient {
    pub client: reqwest::Client,
    pub url: String,
}

impl RpcClient {
    pub fn new(url: String) -> Self {
        RpcClient {
            client: reqwest::Client::new(),
            url,
        }
    }

    pub fn new_socket_with_timeout(addr: SocketAddr, timeout: Duration) -> Self {
        let url = get_rpc_request_str(addr, false);
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("build rpc client");
        RpcClient { client, url }
    }

    pub fn new_socket(addr: SocketAddr) -> Self {
        Self::new(get_rpc_request_str(addr, false))
    }

    pub fn retry_get_balance(
        &self,
        pubkey: &Pubkey,
        retries: usize,
    ) -> Result<Option<u64>, Box<dyn error::Error>> {
        let params = json!([format!("{}", pubkey)]);
        let res = self
            .retry_make_rpc_request(&RpcRequest::GetBalance, Some(params), retries)?
            .as_u64();
        Ok(res)
    }

    pub fn get_account_data(&self, pubkey: &Pubkey) -> io::Result<Vec<u8>> {
        let params = json!([format!("{}", pubkey)]);
        let response = self.make_rpc_request(RpcRequest::GetAccountInfo, Some(params));
        match response {
            Ok(account_json) => {
                let account: Account =
                    serde_json::from_value(account_json).expect("deserialize account");
                Ok(account.data)
            }
            Err(error) => {
                debug!("get_account_data failed: {:?}", error);
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    "get_account_data failed",
                ))
            }
        }
    }

    /// Request the balance of the user holding `pubkey`. This method blocks
    /// until the server sends a response. If the response packet is dropped
    /// by the network, this method will hang indefinitely.
    pub fn get_balance(&self, pubkey: &Pubkey) -> io::Result<u64> {
        let params = json!([format!("{}", pubkey)]);
        let response = self.make_rpc_request(RpcRequest::GetAccountInfo, Some(params));

        response
            .and_then(|account_json| {
                let account: Account =
                    serde_json::from_value(account_json).expect("deserialize account");
                trace!("Response account {:?} {:?}", pubkey, account);
                trace!("get_balance {:?}", account.lamports);
                Ok(account.lamports)
            })
            .map_err(|error| {
                debug!("Response account {}: None (error: {:?})", pubkey, error);
                io::Error::new(io::ErrorKind::Other, "AccountNotFound")
            })
    }

    /// Request the transaction count.  If the response packet is dropped by the network,
    /// this method will try again 5 times.
    pub fn transaction_count(&self) -> u64 {
        debug!("transaction_count");
        for _tries in 0..5 {
            let response = self.make_rpc_request(RpcRequest::GetTransactionCount, None);

            match response {
                Ok(value) => {
                    debug!("transaction_count response: {:?}", value);
                    let transaction_count = value.as_u64().unwrap();
                    return transaction_count;
                }
                Err(error) => {
                    debug!("transaction_count failed: {:?}", error);
                }
            };
        }
        0
    }

    /// Request the last Entry ID from the server without blocking.
    /// Returns the blockhash Hash or None if there was no response from the server.
    pub fn try_get_recent_blockhash(&self, mut num_retries: u64) -> Option<Hash> {
        loop {
            let response = self.make_rpc_request(RpcRequest::GetRecentBlockhash, None);

            match response {
                Ok(value) => {
                    let blockhash_str = value.as_str().unwrap();
                    let blockhash_vec = bs58::decode(blockhash_str).into_vec().unwrap();
                    return Some(Hash::new(&blockhash_vec));
                }
                Err(error) => {
                    debug!("thin_client get_recent_blockhash error: {:?}", error);
                    num_retries -= 1;
                    if num_retries == 0 {
                        return None;
                    }
                }
            }
        }
    }

    /// Request the last Entry ID from the server. This method blocks
    /// until the server sends a response.
    pub fn get_recent_blockhash(&self) -> Hash {
        loop {
            if let Some(hash) = self.try_get_recent_blockhash(10) {
                return hash;
            }
        }
    }

    /// Request a new last Entry ID from the server. This method blocks
    /// until the server sends a response.
    pub fn get_next_blockhash(&self, previous_blockhash: &Hash) -> Hash {
        self.get_next_blockhash_ext(previous_blockhash, &|| {
            sleep(Duration::from_millis(100));
        })
    }

    fn get_next_blockhash_ext(&self, previous_blockhash: &Hash, func: &Fn()) -> Hash {
        loop {
            let blockhash = self.get_recent_blockhash();
            if blockhash != *previous_blockhash {
                break blockhash;
            }
            debug!("Got same blockhash ({:?}), will retry...", blockhash);
            func()
        }
    }

    pub fn poll_balance_with_timeout(
        &self,
        pubkey: &Pubkey,
        polling_frequency: &Duration,
        timeout: &Duration,
    ) -> io::Result<u64> {
        let now = Instant::now();
        loop {
            match self.get_balance(&pubkey) {
                Ok(bal) => {
                    return Ok(bal);
                }
                Err(e) => {
                    sleep(*polling_frequency);
                    if now.elapsed() > *timeout {
                        return Err(e);
                    }
                }
            };
        }
    }

    pub fn poll_get_balance(&self, pubkey: &Pubkey) -> io::Result<u64> {
        self.poll_balance_with_timeout(pubkey, &Duration::from_millis(100), &Duration::from_secs(1))
    }

    /// Poll the server to confirm a transaction.
    pub fn poll_for_signature(&self, signature: &Signature) -> io::Result<()> {
        let now = Instant::now();
        while !self.check_signature(signature) {
            if now.elapsed().as_secs() > 15 {
                // TODO: Return a better error.
                return Err(io::Error::new(io::ErrorKind::Other, "signature not found"));
            }
            sleep(Duration::from_millis(250));
        }
        Ok(())
    }

    /// Check a signature in the bank. This method blocks
    /// until the server sends a response.
    pub fn check_signature(&self, signature: &Signature) -> bool {
        trace!("check_signature: {:?}", signature);
        let params = json!([format!("{}", signature)]);

        loop {
            let response =
                self.make_rpc_request(RpcRequest::ConfirmTransaction, Some(params.clone()));

            match response {
                Ok(confirmation) => {
                    let signature_status = confirmation.as_bool().unwrap();
                    if signature_status {
                        trace!("Response found signature");
                    } else {
                        trace!("Response signature not found");
                    }

                    return signature_status;
                }
                Err(err) => {
                    debug!("check_signature request failed: {:?}", err);
                }
            };
        }
    }
    pub fn fullnode_exit(&self) -> io::Result<bool> {
        let response = self
            .make_rpc_request(RpcRequest::FullnodeExit, None)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("FullnodeExit request failure: {:?}", err),
                )
            })?;
        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("FullnodeExit parse failure: {:?}", err),
            )
        })
    }

    pub fn retry_make_rpc_request(
        &self,
        request: &RpcRequest,
        params: Option<Value>,
        mut retries: usize,
    ) -> Result<Value, Box<dyn error::Error>> {
        // Concurrent requests are not supported so reuse the same request id for all requests
        let request_id = 1;

        let request_json = request.build_request_json(request_id, params);

        loop {
            match self
                .client
                .post(&self.url)
                .header(CONTENT_TYPE, "application/json")
                .body(request_json.to_string())
                .send()
            {
                Ok(mut response) => {
                    let json: Value = serde_json::from_str(&response.text()?)?;
                    if json["error"].is_object() {
                        Err(RpcError::RpcRequestError(format!(
                            "RPC Error response: {}",
                            serde_json::to_string(&json["error"]).unwrap()
                        )))?
                    }
                    return Ok(json["result"].clone());
                }
                Err(e) => {
                    info!(
                        "make_rpc_request() failed, {} retries left: {:?}",
                        retries, e
                    );
                    if retries == 0 {
                        Err(e)?;
                    }
                    retries -= 1;

                    // Sleep for approximately half a slot
                    sleep(Duration::from_millis(
                        500 * DEFAULT_TICKS_PER_SLOT / NUM_TICKS_PER_SECOND,
                    ));
                }
            }
        }
    }
}

pub fn get_rpc_request_str(rpc_addr: SocketAddr, tls: bool) -> String {
    if tls {
        format!("https://{}", rpc_addr)
    } else {
        format!("http://{}", rpc_addr)
    }
}

pub trait RpcRequestHandler {
    fn make_rpc_request(
        &self,
        request: RpcRequest,
        params: Option<Value>,
    ) -> Result<Value, Box<dyn error::Error>>;
}

impl RpcRequestHandler for RpcClient {
    fn make_rpc_request(
        &self,
        request: RpcRequest,
        params: Option<Value>,
    ) -> Result<Value, Box<dyn error::Error>> {
        self.retry_make_rpc_request(&request, params, 0)
    }
}

#[derive(Debug, PartialEq)]
pub enum RpcRequest {
    ConfirmTransaction,
    GetAccountInfo,
    GetBalance,
    GetRecentBlockhash,
    GetSignatureStatus,
    GetTransactionCount,
    RequestAirdrop,
    SendTransaction,
    RegisterNode,
    SignVote,
    DeregisterNode,
    GetStorageBlockhash,
    GetStorageEntryHeight,
    GetStoragePubkeysForEntryHeight,
    FullnodeExit,
}

impl RpcRequest {
    fn build_request_json(&self, id: u64, params: Option<Value>) -> Value {
        let jsonrpc = "2.0";
        let method = match self {
            RpcRequest::ConfirmTransaction => "confirmTransaction",
            RpcRequest::GetAccountInfo => "getAccountInfo",
            RpcRequest::GetBalance => "getBalance",
            RpcRequest::GetRecentBlockhash => "getRecentBlockhash",
            RpcRequest::GetSignatureStatus => "getSignatureStatus",
            RpcRequest::GetTransactionCount => "getTransactionCount",
            RpcRequest::RequestAirdrop => "requestAirdrop",
            RpcRequest::SendTransaction => "sendTransaction",
            RpcRequest::RegisterNode => "registerNode",
            RpcRequest::SignVote => "signVote",
            RpcRequest::DeregisterNode => "deregisterNode",
            RpcRequest::GetStorageBlockhash => "getStorageBlockhash",
            RpcRequest::GetStorageEntryHeight => "getStorageEntryHeight",
            RpcRequest::GetStoragePubkeysForEntryHeight => "getStoragePubkeysForEntryHeight",
            RpcRequest::FullnodeExit => "fullnodeExit",
        };
        let mut request = json!({
           "jsonrpc": jsonrpc,
           "id": id,
           "method": method,
        });
        if let Some(param_string) = params {
            request["params"] = param_string;
        }
        request
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RpcError {
    RpcRequestError(String),
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid")
    }
}

impl error::Error for RpcError {
    fn description(&self) -> &str {
        "invalid"
    }

    fn cause(&self) -> Option<&dyn error::Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonrpc_core::{Error, IoHandler, Params};
    use jsonrpc_http_server::{AccessControlAllowOrigin, DomainsValidation, ServerBuilder};
    use serde_json::Number;
    use solana_logger;
    use std::sync::mpsc::channel;
    use std::thread;

    #[test]
    fn test_build_request_json() {
        let test_request = RpcRequest::GetAccountInfo;
        let addr = json!(["deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx"]);
        let request = test_request.build_request_json(1, Some(addr.clone()));
        assert_eq!(request["method"], "getAccountInfo");
        assert_eq!(request["params"], addr,);

        let test_request = RpcRequest::GetBalance;
        let request = test_request.build_request_json(1, Some(addr));
        assert_eq!(request["method"], "getBalance");

        let test_request = RpcRequest::GetRecentBlockhash;
        let request = test_request.build_request_json(1, None);
        assert_eq!(request["method"], "getRecentBlockhash");

        let test_request = RpcRequest::GetTransactionCount;
        let request = test_request.build_request_json(1, None);
        assert_eq!(request["method"], "getTransactionCount");

        let test_request = RpcRequest::RequestAirdrop;
        let request = test_request.build_request_json(1, None);
        assert_eq!(request["method"], "requestAirdrop");

        let test_request = RpcRequest::SendTransaction;
        let request = test_request.build_request_json(1, None);
        assert_eq!(request["method"], "sendTransaction");
    }
    #[test]
    fn test_make_rpc_request() {
        let (sender, receiver) = channel();
        thread::spawn(move || {
            let rpc_addr = "0.0.0.0:0".parse().unwrap();
            let mut io = IoHandler::default();
            // Successful request
            io.add_method("getBalance", |_params: Params| {
                Ok(Value::Number(Number::from(50)))
            });
            // Failed request
            io.add_method("getRecentBlockhash", |params: Params| {
                if params != Params::None {
                    Err(Error::invalid_request())
                } else {
                    Ok(Value::String(
                        "deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx".to_string(),
                    ))
                }
            });

            let server = ServerBuilder::new(io)
                .threads(1)
                .cors(DomainsValidation::AllowOnly(vec![
                    AccessControlAllowOrigin::Any,
                ]))
                .start_http(&rpc_addr)
                .expect("Unable to start RPC server");
            sender.send(*server.address()).unwrap();
            server.wait();
        });

        let rpc_addr = receiver.recv().unwrap();
        let rpc_client = RpcClient::new_socket(rpc_addr);

        let balance = rpc_client.make_rpc_request(
            RpcRequest::GetBalance,
            Some(json!(["deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx"])),
        );
        assert_eq!(balance.unwrap().as_u64().unwrap(), 50);

        let blockhash = rpc_client.make_rpc_request(RpcRequest::GetRecentBlockhash, None);
        assert_eq!(
            blockhash.unwrap().as_str().unwrap(),
            "deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx"
        );

        // Send erroneous parameter
        let blockhash =
            rpc_client.make_rpc_request(RpcRequest::GetRecentBlockhash, Some(json!("paramter")));
        assert_eq!(blockhash.is_err(), true);
    }

    #[test]
    fn test_retry_make_rpc_request() {
        solana_logger::setup();
        let (sender, receiver) = channel();
        thread::spawn(move || {
            // 1. Pick a random port
            // 2. Tell the client to start using it
            // 3. Delay for 1.5 seconds before starting the server to ensure the client will fail
            //    and need to retry
            let rpc_addr: SocketAddr = "0.0.0.0:4242".parse().unwrap();
            sender.send(rpc_addr.clone()).unwrap();
            sleep(Duration::from_millis(1500));

            let mut io = IoHandler::default();
            io.add_method("getBalance", move |_params: Params| {
                Ok(Value::Number(Number::from(5)))
            });
            let server = ServerBuilder::new(io)
                .threads(1)
                .cors(DomainsValidation::AllowOnly(vec![
                    AccessControlAllowOrigin::Any,
                ]))
                .start_http(&rpc_addr)
                .expect("Unable to start RPC server");
            server.wait();
        });

        let rpc_addr = receiver.recv().unwrap();
        let rpc_client = RpcClient::new_socket(rpc_addr);

        let balance = rpc_client.retry_make_rpc_request(
            &RpcRequest::GetBalance,
            Some(json!(["deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhw"])),
            10,
        );
        assert_eq!(balance.unwrap().as_u64().unwrap(), 5);
    }
}

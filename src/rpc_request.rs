use reqwest;
use reqwest::header::CONTENT_TYPE;
use serde_json::{self, Value};
use std::net::SocketAddr;
use std::thread::sleep;
use std::time::Duration;
use std::{error, fmt};

#[derive(Clone)]
pub struct RpcClient {
    pub client: reqwest::Client,
    pub addr: String,
}

impl RpcClient {
    pub fn new(addr: String) -> Self {
        RpcClient {
            client: reqwest::Client::new(),
            addr,
        }
    }

    pub fn new_with_timeout(addr: SocketAddr, timeout: Duration) -> Self {
        let addr = get_rpc_request_str(addr);
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("build rpc client");
        RpcClient { client, addr }
    }

    pub fn new_from_socket(addr: SocketAddr) -> Self {
        let addr = get_rpc_request_str(addr);
        RpcClient {
            client: reqwest::Client::new(),
            addr,
        }
    }
}

pub fn get_rpc_request_str(rpc_addr: SocketAddr) -> String {
    format!("http://{}", rpc_addr)
}

pub trait RpcRequestHandler {
    fn make_rpc_request(
        &self,
        client: &RpcClient,
        id: u64,
        params: Option<Value>,
    ) -> Result<Value, Box<dyn error::Error>>;
}

pub struct Rpu(pub RpcRequest);

#[derive(Debug, PartialEq)]
pub enum RpcRequest {
    ConfirmTransaction,
    GetAccountInfo,
    GetBalance,
    GetConfirmationTime,
    GetLastId,
    GetSignatureStatus,
    GetTransactionCount,
    RequestAirdrop,
    SendTransaction,
    RegisterNode,
    SignVote,
    DeregisterNode,
    GetStorageMiningLastId,
    GetStorageMiningEntryHeight,
    GetStoragePubkeysForEntryHeight,
}

impl RpcRequestHandler for Rpu {
    fn make_rpc_request(
        &self,
        client: &RpcClient,
        id: u64,
        params: Option<Value>,
    ) -> Result<Value, Box<dyn error::Error>> {
        self.retry_make_rpc_request(client, id, params, 0)
    }
}

impl Rpu {
    pub fn retry_make_rpc_request(
        &self,
        client: &RpcClient,
        id: u64,
        params: Option<Value>,
        mut retries: usize,
    ) -> Result<Value, Box<dyn error::Error>> {
        let request = self.build_request_json(id, params);

        loop {
            match client
                .client
                .post(&client.addr)
                .header(CONTENT_TYPE, "application/json")
                .body(request.to_string())
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
                    // TODO: Make the caller supply their desired retry frequency?
                    sleep(Duration::from_millis(500));
                }
            }
        }
    }

    fn build_request_json(&self, id: u64, params: Option<Value>) -> Value {
        let jsonrpc = "2.0";
        let method = match self.0 {
            RpcRequest::ConfirmTransaction => "confirmTransaction",
            RpcRequest::GetAccountInfo => "getAccountInfo",
            RpcRequest::GetBalance => "getBalance",
            RpcRequest::GetConfirmationTime => "getConfirmationTime",
            RpcRequest::GetLastId => "getLastId",
            RpcRequest::GetSignatureStatus => "getSignatureStatus",
            RpcRequest::GetTransactionCount => "getTransactionCount",
            RpcRequest::RequestAirdrop => "requestAirdrop",
            RpcRequest::SendTransaction => "sendTransaction",
            RpcRequest::RegisterNode => "registerNode",
            RpcRequest::SignVote => "signVote",
            RpcRequest::DeregisterNode => "deregisterNode",
            RpcRequest::GetStorageMiningLastId => "getStorageMiningLastId",
            RpcRequest::GetStorageMiningEntryHeight => "getStorageMiningEntryHeight",
            RpcRequest::GetStoragePubkeysForEntryHeight => "getStoragePubkeysForEntryHeight",
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
    use crate::jsonrpc_core::*;
    use crate::jsonrpc_http_server::*;
    use serde_json::Number;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::sync::mpsc::channel;
    use std::thread;

    #[test]
    fn test_build_request_json() {
        let test_request = Rpu(RpcRequest::GetAccountInfo);
        let request = test_request.build_request_json(
            1,
            Some(json!(["deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx"])),
        );
        assert_eq!(request["method"], "getAccountInfo");
        assert_eq!(
            request["params"],
            json!(["deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx"])
        );

        let test_request = Rpu(RpcRequest::GetBalance);
        let request = test_request.build_request_json(
            1,
            Some(json!(["deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx"])),
        );
        assert_eq!(request["method"], "getBalance");

        let test_request = Rpu(RpcRequest::GetConfirmationTime);
        let request = test_request.build_request_json(1, None);
        assert_eq!(request["method"], "getConfirmationTime");
        assert_eq!(request["params"], json!(null));

        let test_request = Rpu(RpcRequest::GetLastId);
        let request = test_request.build_request_json(1, None);
        assert_eq!(request["method"], "getLastId");

        let test_request = Rpu(RpcRequest::GetTransactionCount);
        let request = test_request.build_request_json(1, None);
        assert_eq!(request["method"], "getTransactionCount");

        let test_request = Rpu(RpcRequest::RequestAirdrop);
        let request = test_request.build_request_json(1, None);
        assert_eq!(request["method"], "requestAirdrop");

        let test_request = Rpu(RpcRequest::SendTransaction);
        let request = test_request.build_request_json(1, None);
        assert_eq!(request["method"], "sendTransaction");
    }
    #[test]
    fn test_make_rpc_request() {
        let (sender, receiver) = channel();
        thread::spawn(move || {
            let rpc_addr = socketaddr!(0, 0);
            let mut io = IoHandler::default();
            // Successful request
            io.add_method("getBalance", |_params: Params| {
                Ok(Value::Number(Number::from(50)))
            });
            // Failed request
            io.add_method("getLastId", |params: Params| {
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
        let rpc_client = RpcClient::new_from_socket(rpc_addr);

        let balance = Rpu(RpcRequest::GetBalance).make_rpc_request(
            &rpc_client,
            1,
            Some(json!(["deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx"])),
        );
        assert!(balance.is_ok());
        assert_eq!(balance.unwrap().as_u64().unwrap(), 50);

        let last_id = Rpu(RpcRequest::GetLastId).make_rpc_request(&rpc_client, 2, None);
        assert!(last_id.is_ok());
        assert_eq!(
            last_id.unwrap().as_str().unwrap(),
            "deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx"
        );

        // Send erroneous parameter
        let last_id =
            Rpu(RpcRequest::GetLastId).make_rpc_request(&rpc_client, 3, Some(json!("paramter")));
        assert_eq!(last_id.is_err(), true);
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
            let rpc_addr = socketaddr!(0, 4242);
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
        let rpc_client = RpcClient::new_from_socket(rpc_addr);

        let balance = Rpu(RpcRequest::GetBalance).retry_make_rpc_request(
            &rpc_client,
            1,
            Some(json!(["deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhw"])),
            10,
        );
        assert!(balance.is_ok());
        assert_eq!(balance.unwrap().as_u64().unwrap(), 5);
    }
}

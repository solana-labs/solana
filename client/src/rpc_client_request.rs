use crate::generic_rpc_client_request::GenericRpcClientRequest;
use crate::rpc_request::{RpcError, RpcRequest};
use log::*;
use reqwest;
use reqwest::header::CONTENT_TYPE;
use solana_sdk::timing::{DEFAULT_TICKS_PER_SLOT, NUM_TICKS_PER_SECOND};
use std::thread::sleep;
use std::time::Duration;

pub struct RpcClientRequest {
    client: reqwest::Client,
    url: String,
}

impl RpcClientRequest {
    pub fn new(url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            url,
        }
    }

    pub fn new_with_timeout(url: String, timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("build rpc client");

        Self { client, url }
    }
}

impl GenericRpcClientRequest for RpcClientRequest {
    fn send(
        &self,
        request: &RpcRequest,
        params: Option<serde_json::Value>,
        mut retries: usize,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
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
                    let json: serde_json::Value = serde_json::from_str(&response.text()?)?;
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
                        "make_rpc_request({:?}) failed, {} retries left: {:?}",
                        request, retries, e
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

use crate::{
    client_error::ClientError,
    generic_rpc_client_request::GenericRpcClientRequest,
    rpc_request::{RpcError, RpcRequest},
};
use log::*;
use reqwest::{self, header::CONTENT_TYPE};
use solana_sdk::{
    clock::{DEFAULT_TICKS_PER_SECOND, DEFAULT_TICKS_PER_SLOT},
    commitment_config::CommitmentConfig,
};
use std::{thread::sleep, time::Duration};

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
        commitment_config: Option<CommitmentConfig>,
    ) -> Result<serde_json::Value, ClientError> {
        // Concurrent requests are not supported so reuse the same request id for all requests
        let request_id = 1;

        let request_json = request.build_request_json(request_id, params, commitment_config);

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
                        return Err(RpcError::RpcRequestError(format!(
                            "RPC Error response: {}",
                            serde_json::to_string(&json["error"]).unwrap()
                        ))
                        .into());
                    }
                    return Ok(json["result"].clone());
                }
                Err(e) => {
                    info!("{:?} failed, {} retries left: {:?}", request, retries, e);
                    if retries == 0 {
                        return Err(e.into());
                    }
                    retries -= 1;

                    // Sleep for approximately half a slot
                    sleep(Duration::from_millis(
                        500 * DEFAULT_TICKS_PER_SLOT / DEFAULT_TICKS_PER_SECOND,
                    ));
                }
            }
        }
    }
}

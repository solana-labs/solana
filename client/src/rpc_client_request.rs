use crate::client_error::ClientError;
use crate::generic_rpc_client_request::GenericRpcClientRequest;
use crate::rpc_request::{RpcError, RpcRequest};
use log::*;
use solana_sdk::timing::{DEFAULT_TICKS_PER_SECOND, DEFAULT_TICKS_PER_SLOT};
use std::io::{Error as IoError, ErrorKind};
use std::thread::sleep;
use std::time::Duration;
use ureq::Error;

pub struct RpcClientRequest {
    client: ureq::Agent,
    url: String,
    timeout: Option<Duration>,
}

impl RpcClientRequest {
    pub fn new(url: String) -> Self {
        Self {
            client: ureq::agent(),
            url,
            timeout: None,
        }
    }

    // Timeout applies separately to each socket action: connect, read, write
    pub fn new_with_timeout(url: String, timeout: Duration) -> Self {
        Self {
            client: ureq::agent(),
            url,
            timeout: Some(timeout),
        }
    }
}

impl GenericRpcClientRequest for RpcClientRequest {
    fn send(
        &self,
        request: &RpcRequest,
        params: Option<serde_json::Value>,
        mut retries: usize,
    ) -> Result<serde_json::Value, ClientError> {
        // Concurrent requests are not supported so reuse the same request id for all requests
        let request_id = 1;

        let request_json = request.build_request_json(request_id, params);

        loop {
            let mut request_builder = self
                .client
                .post(&self.url)
                .set("Content-Type", "application/json")
                .build();
            if self.timeout.is_some() {
                request_builder = request_builder
                    .timeout_connect(self.timeout.unwrap().as_millis() as u64)
                    .timeout_read(self.timeout.unwrap().as_millis() as u64)
                    .timeout_write(self.timeout.unwrap().as_millis() as u64)
                    .build();
            }
            let response = request_builder.send_json(request_json.clone());
            if response.ok() {
                let json: serde_json::Value = response.into_json()?;
                if json["error"].is_object() {
                    Err(RpcError::RpcRequestError(format!(
                        "RPC Error response: {}",
                        serde_json::to_string(&json["error"]).unwrap()
                    )))?
                }
                return Ok(json["result"].clone());
            } else {
                let io_error = Error::Io(IoError::new(ErrorKind::Other, "Unspecified error"));
                let error = if let Some(err) = response.synthetic_error().as_ref() {
                    err
                } else {
                    &io_error
                };
                info!(
                    "make_rpc_request({:?}) failed, {} retries left: {:?}",
                    request, retries, error
                );
                if retries == 0 {
                    Err(error)?;
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

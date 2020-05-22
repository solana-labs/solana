use crate::{
    client_error::Result,
    rpc_request::{RpcError, RpcRequest},
    rpc_sender::RpcSender,
};
use log::*;
use reqwest::{self, header::CONTENT_TYPE, StatusCode};
use std::{thread::sleep, time::Duration};

pub struct HttpSender {
    client: reqwest::blocking::Client,
    url: String,
}

impl HttpSender {
    pub fn new(url: String) -> Self {
        Self::new_with_timeout(url, Duration::from_secs(30))
    }

    pub fn new_with_timeout(url: String, timeout: Duration) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .expect("build rpc client");

        Self { client, url }
    }
}

impl RpcSender for HttpSender {
    fn send(&self, request: RpcRequest, params: serde_json::Value) -> Result<serde_json::Value> {
        // Concurrent requests are not supported so reuse the same request id for all requests
        let request_id = 1;

        let request_json = request.build_request_json(request_id, params);

        let mut too_many_requests_retries = 5;
        loop {
            match self
                .client
                .post(&self.url)
                .header(CONTENT_TYPE, "application/json")
                .body(request_json.to_string())
                .send()
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        if response.status() == StatusCode::TOO_MANY_REQUESTS
                            && too_many_requests_retries > 0
                        {
                            too_many_requests_retries -= 1;
                            debug!(
                                "Server responded with {:?}, {} retries left",
                                response, too_many_requests_retries
                            );

                            // Sleep for 500ms to give the server a break
                            sleep(Duration::from_millis(500));
                            continue;
                        }
                        return Err(response.error_for_status().unwrap_err().into());
                    }

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
                Err(err) => {
                    return Err(err.into());
                }
            }
        }
    }
}

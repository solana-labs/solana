//! A transport for RPC calls.
use {
    async_trait::async_trait,
    solana_rpc_client_api::{client_error::Result, request::RpcRequest},
    std::time::Duration,
};

#[derive(Default, Clone)]
pub struct RpcTransportStats {
    /// Number of RPC requests issued
    pub request_count: usize,

    /// Total amount of time spent transacting with the RPC server
    pub elapsed_time: Duration,

    /// Total amount of waiting time due to RPC server rate limiting
    /// (a subset of `elapsed_time`)
    pub rate_limited_time: Duration,
}

/// A transport for RPC calls.
///
/// `RpcSender` implements the underlying transport of requests to, and
/// responses from, a Solana node, and is used primarily by [`RpcClient`].
///
/// [`RpcClient`]: crate::rpc_client::RpcClient
#[async_trait]
pub trait RpcSender {
    async fn send(
        &self,
        request: RpcRequest,
        params: serde_json::Value,
    ) -> Result<serde_json::Value>;
    fn get_transport_stats(&self) -> RpcTransportStats;
    fn url(&self) -> String;
}

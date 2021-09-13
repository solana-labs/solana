//! A transport for RPC calls.

use {
    crate::{client_error::Result, rpc_request::RpcRequest},
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
/// It is typically implemented by [`HttpSender`] in production, and
/// [`MockSender`] in unit tests.
///
/// [`RpcClient`]: crate::rpc_client::RpcClient
/// [`HttpSender`]: crate::http_sender::HttpSender
/// [`MockSender`]: crate::mock_sender::MockSender
pub trait RpcSender {
    fn send(&self, request: RpcRequest, params: serde_json::Value) -> Result<serde_json::Value>;
    fn get_transport_stats(&self) -> RpcTransportStats;
}

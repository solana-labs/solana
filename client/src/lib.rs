#![allow(clippy::integer_arithmetic)]
#[macro_use]
extern crate serde_derive;

pub mod blockhash_query;
pub mod client_error;
pub mod connection_cache;
pub(crate) mod http_sender;
pub(crate) mod mock_sender;
pub mod nonblocking;
pub mod nonce_utils;
pub mod pubsub_client;
pub mod quic_client;
pub mod rpc_cache;
pub mod rpc_client;
pub mod rpc_config;
pub mod rpc_custom_error;
pub mod rpc_deprecated_config;
pub mod rpc_filter;
pub mod rpc_request;
pub mod rpc_response;
pub mod rpc_sender;
pub mod spinner;
pub mod thin_client;
pub mod tpu_client;
pub mod tpu_connection;
pub mod transaction_executor;
pub mod udp_client;

pub mod mock_sender_for_cli {
    /// Magic `SIGNATURE` value used by `solana-cli` unit tests.
    /// Please don't use this constant.
    pub const SIGNATURE: &str =
        "43yNSFC6fYTuPgTNFFhF4axw7AfWxB2BPdurme8yrsWEYwm8299xh8n6TAHjGymiSub1XtyxTNyd9GBfY2hxoBw8";
}

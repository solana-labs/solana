//! Communication with a Solana node over RPC asynchronously .
//!
//! Software that interacts with the Solana blockchain, whether querying its
//! state or submitting transactions, communicates with a Solana node over
//! [JSON-RPC], using the [`RpcClient`] type.
//!
//! [JSON-RPC]: https://www.jsonrpc.org/specification

pub use solana_rpc_client::nonblocking::rpc_client::*;

#![allow(clippy::integer_arithmetic)]

pub mod transaction_executor;

pub mod blockhash_query {
    pub use solana_nonce_client::blockhash_query::*;
}
pub mod client_error {
    pub use solana_rpc_client_api::client_error::{
        reqwest, Error as ClientError, ErrorKind as ClientErrorKind, Result,
    };
}
pub mod connection_cache {
    pub use solana_tpu_client::connection_cache::*;
}
pub mod nonblocking {
    pub mod blockhash_query {
        pub use solana_nonce_client::nonblocking::blockhash_query::*;
    }
    /// Durable transaction nonce helpers.
    pub mod nonce_utils {
        pub use solana_nonce_client::nonblocking::nonce_utils::*;
    }
    pub mod pubsub_client {
        pub use solana_pubsub_client::nonblocking::pubsub_client::*;
    }
    /// Simple nonblocking client that connects to a given UDP port with the QUIC protocol
    /// and provides an interface for sending transactions which is restricted by the
    /// server's flow control.
    pub mod quic_client {
        pub use solana_tpu_client::nonblocking::quic_client::*;
    }
    /// Communication with a Solana node over RPC asynchronously .
    ///
    /// Software that interacts with the Solana blockchain, whether querying its
    /// state or submitting transactions, communicates with a Solana node over
    /// [JSON-RPC], using the [`RpcClient`] type.
    ///
    /// [JSON-RPC]: https://www.jsonrpc.org/specification
    pub mod rpc_client {
        pub use solana_rpc_client::nonblocking::rpc_client::*;
    }
    pub mod tpu_client {
        pub use solana_tpu_client::nonblocking::tpu_client::*;
    }
    /// Trait defining async send functions, to be used for UDP or QUIC sending
    pub mod tpu_connection {
        pub use solana_tpu_client::nonblocking::tpu_connection::*;
    }
    /// Simple UDP client that communicates with the given UDP port with UDP and provides
    /// an interface for sending transactions
    pub mod udp_client {
        pub use solana_tpu_client::nonblocking::udp_client::*;
    }
}
/// Durable transaction nonce helpers.
pub mod nonce_utils {
    pub use solana_nonce_client::nonce_utils::*;
}
pub mod pubsub_client {
    pub use solana_pubsub_client::pubsub_client::*;
}
/// Simple client that connects to a given UDP port with the QUIC protocol and provides
/// an interface for sending transactions which is restricted by the server's flow control.
pub mod quic_client {
    pub use solana_tpu_client::quic_client::*;
}
/// Communication with a Solana node over RPC.
///
/// Software that interacts with the Solana blockchain, whether querying its
/// state or submitting transactions, communicates with a Solana node over
/// [JSON-RPC], using the [`RpcClient`] type.
///
/// [JSON-RPC]: https://www.jsonrpc.org/specification
pub mod rpc_client {
    pub use solana_rpc_client::rpc_client::*;
}
pub mod rpc_config {
    pub use solana_rpc_client_api::config::*;
}
/// Implementation defined RPC server errors
pub mod rpc_custom_error {
    pub use solana_rpc_client_api::custom_error::*;
}
pub mod rpc_deprecated_config {
    pub use solana_rpc_client_api::deprecated_config::*;
}
pub mod rpc_filter {
    pub use solana_rpc_client_api::filter::*;
}
pub mod rpc_request {
    pub use solana_rpc_client_api::request::*;
}
pub mod rpc_response {
    pub use solana_rpc_client_api::response::*;
}
/// A transport for RPC calls.
pub mod rpc_sender {
    pub use solana_rpc_client::rpc_sender::*;
}
/// The `thin_client` module is a client-side object that interfaces with
/// a server-side TPU.  Client code should use this object instead of writing
/// messages to the network directly. The binary encoding of its messages are
/// unstable and may change in future releases.
pub mod thin_client {
    pub use solana_thin_client::thin_client::*;
}
pub mod tpu_client {
    pub use solana_tpu_client::tpu_client::*;
}
pub mod tpu_connection {
    pub use solana_tpu_client::tpu_connection::*;
}
/// Simple TPU client that communicates with the given UDP port with UDP and provides
/// an interface for sending transactions
pub mod udp_client {
    pub use solana_tpu_client::udp_client::*;
}

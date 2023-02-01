pub use solana_quic_client::nonblocking::quic_client::{
    QuicClient, QuicClientCertificate, QuicLazyInitializedEndpoint,
};

#[deprecated(since = "1.15.0", note = "Please use `QuicClientConnection` instead.")]
pub use solana_quic_client::nonblocking::quic_client::QuicClientConnection as QuicTpuConnection;

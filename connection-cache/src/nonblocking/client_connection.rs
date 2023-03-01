//! Trait defining async send functions, to be used for UDP or QUIC sending

use {
    async_trait::async_trait, solana_sdk::transport::Result as TransportResult,
    std::net::SocketAddr,
};

#[async_trait]
pub trait ClientConnection {
    fn server_addr(&self) -> &SocketAddr;

    async fn send_data(&self, buffer: &[u8]) -> TransportResult<()>;

    async fn send_data_batch(&self, buffers: &[Vec<u8>]) -> TransportResult<()>;
}

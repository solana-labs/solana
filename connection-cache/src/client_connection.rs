use {
    solana_metrics::MovingStat,
    solana_sdk::transport::Result as TransportResult,
    std::{net::SocketAddr, sync::atomic::AtomicU64},
};

#[derive(Default)]
pub struct ClientStats {
    pub total_connections: AtomicU64,
    pub connection_reuse: AtomicU64,
    pub connection_errors: AtomicU64,
    pub zero_rtt_accepts: AtomicU64,
    pub zero_rtt_rejects: AtomicU64,

    // these will be the last values of these stats
    pub congestion_events: MovingStat,
    pub streams_blocked_uni: MovingStat,
    pub data_blocked: MovingStat,
    pub acks: MovingStat,
    pub make_connection_ms: AtomicU64,
    pub send_timeout: AtomicU64,
}

pub trait ClientConnection: Sync + Send {
    fn server_addr(&self) -> &SocketAddr;

    fn send_data(&self, buffer: &[u8]) -> TransportResult<()>;

    fn send_data_async(&self, buffer: Vec<u8>) -> TransportResult<()>;

    fn send_data_batch(&self, buffers: &[Vec<u8>]) -> TransportResult<()>;

    fn send_data_batch_async(&self, buffers: Vec<Vec<u8>>) -> TransportResult<()>;
}

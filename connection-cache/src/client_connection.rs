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
    /// The time spent sending packets when packets are successfully sent. This include both time
    /// preparing for a connection (either obtaining from cache or create a new one in case of cache miss
    /// or connection error)
    pub send_packets_us: AtomicU64,
    /// `prepare_connection_us` differs from `make_connection_ms` in that it accounts for the time spent
    /// on obtaining a successful connection including time spent on retries when sending a packet.
    pub prepare_connection_us: AtomicU64,
    /// Count of packets successfully sent
    pub successful_packets: AtomicU64,
}

pub trait ClientConnection: Sync + Send {
    fn server_addr(&self) -> &SocketAddr;

    fn send_data(&self, buffer: &[u8]) -> TransportResult<()>;

    fn send_data_async(&self, buffer: Vec<u8>) -> TransportResult<()>;

    fn send_data_batch(&self, buffers: &[Vec<u8>]) -> TransportResult<()>;

    fn send_data_batch_async(&self, buffers: Vec<Vec<u8>>) -> TransportResult<()>;
}

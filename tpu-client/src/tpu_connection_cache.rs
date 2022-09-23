use {
    crate::{
        connection_cache_stats::ConnectionCacheStats,
        nonblocking::tpu_connection::TpuConnection as NonblockingTpuConnection,
        tpu_connection::TpuConnection as BlockingTpuConnection,
    },
    rand::{thread_rng, Rng},
    std::{net::SocketAddr, sync::Arc},
    thiserror::Error,
};

// Should be non-zero
pub(crate) static MAX_CONNECTIONS: usize = 1024;

/// Used to decide whether the TPU and underlying connection cache should use
/// QUIC connections.
pub const DEFAULT_TPU_USE_QUIC: bool = true;

/// Default TPU connection pool size per remote address
pub const DEFAULT_TPU_CONNECTION_POOL_SIZE: usize = 4;

pub const DEFAULT_TPU_ENABLE_UDP: bool = false;

#[derive(Error, Debug)]
pub enum ConnectionPoolError {
    #[error("connection index is out of range of the pool")]
    IndexOutOfRange,
}

pub trait ConnectionPool {
    type PoolTpuConnection: BaseTpuConnection;
    type TpuConfig: Default;
    const PORT_OFFSET: u16 = 0;

    /// Create a new connection pool based on protocol-specific configuration
    fn new_with_connection(config: &Self::TpuConfig, addr: &SocketAddr) -> Self;

    /// Add a connection to the pool
    fn add_connection(&mut self, config: &Self::TpuConfig, addr: &SocketAddr);

    /// Get the number of current connections in the pool
    fn num_connections(&self) -> usize;

    /// Get a connection based on its index in the pool, without checking if the
    fn get(&self, index: usize) -> Result<Arc<Self::PoolTpuConnection>, ConnectionPoolError>;

    /// Get a connection from the pool. It must have at least one connection in the pool.
    /// This randomly picks a connection in the pool.
    fn borrow_connection(&self) -> Arc<Self::PoolTpuConnection> {
        let mut rng = thread_rng();
        let n = rng.gen_range(0, self.num_connections());
        self.get(n).expect("index is within num_connections")
    }
    /// Check if we need to create a new connection. If the count of the connections
    /// is smaller than the pool size.
    fn need_new_connection(&self, required_pool_size: usize) -> bool {
        self.num_connections() < required_pool_size
    }

    fn create_pool_entry(
        &self,
        config: &Self::TpuConfig,
        addr: &SocketAddr,
    ) -> Self::PoolTpuConnection;
}

pub trait BaseTpuConnection {
    type BlockingConnectionType: BlockingTpuConnection;
    type NonblockingConnectionType: NonblockingTpuConnection;

    fn new_blocking_connection(
        &self,
        addr: SocketAddr,
        stats: Arc<ConnectionCacheStats>,
    ) -> Self::BlockingConnectionType;

    fn new_nonblocking_connection(
        &self,
        addr: SocketAddr,
        stats: Arc<ConnectionCacheStats>,
    ) -> Self::NonblockingConnectionType;
}

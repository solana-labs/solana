use {
    crate::{
        connection_cache::ConnectionCacheStats,
        nonblocking::tpu_connection::TpuConnection as NonblockingTpuConnection,
        tpu_connection::TpuConnection as BlockingTpuConnection,
    },
    std::{net::SocketAddr, sync::Arc},
};

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

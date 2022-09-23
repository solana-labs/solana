#![allow(clippy::integer_arithmetic)]

pub mod nonblocking;

use {
    solana_tpu_client::{
        connection_cache::ConnectionCacheStats,
        nonblocking::quic_client::{QuicClient, QuicTpuConnection as NonblockingQuicTpuConnection},
        quic_client::QuicTpuConnection as BlockingQuicTpuConnection,
        tpu_connection_cache::BaseTpuConnection,
    },
    std::{net::SocketAddr, sync::Arc},
};

pub struct Quic(Arc<QuicClient>);
impl BaseTpuConnection for Quic {
    type BlockingConnectionType = BlockingQuicTpuConnection;
    type NonblockingConnectionType = NonblockingQuicTpuConnection;

    fn new_blocking_connection(
        &self,
        _addr: SocketAddr,
        stats: Arc<ConnectionCacheStats>,
    ) -> BlockingQuicTpuConnection {
        BlockingQuicTpuConnection::new_with_client(self.0.clone(), stats)
    }

    fn new_nonblocking_connection(
        &self,
        _addr: SocketAddr,
        stats: Arc<ConnectionCacheStats>,
    ) -> NonblockingQuicTpuConnection {
        NonblockingQuicTpuConnection::new_with_client(self.0.clone(), stats)
    }
}

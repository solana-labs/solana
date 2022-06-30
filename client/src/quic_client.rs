//! Simple client that connects to a given UDP port with the QUIC protocol and provides
//! an interface for sending transactions which is restricted by the server's flow control.

use {
    crate::{
        connection_cache::ConnectionCacheStats,
        nonblocking::{
            quic_client::{
                QuicClient, QuicLazyInitializedEndpoint,
                QuicTpuConnection as NonblockingQuicTpuConnection,
            },
            tpu_connection::TpuConnection as NonblockingTpuConnection,
        },
        tpu_connection::TpuConnection,
    },
    lazy_static::lazy_static,
    solana_sdk::transport::Result as TransportResult,
    std::{net::SocketAddr, sync::Arc},
    tokio::runtime::Runtime,
};

lazy_static! {
    static ref RUNTIME: Runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
}

pub struct QuicTpuConnection {
    inner: Arc<NonblockingQuicTpuConnection>,
}
impl QuicTpuConnection {
    pub fn new(
        endpoint: Arc<QuicLazyInitializedEndpoint>,
        tpu_addr: SocketAddr,
        connection_stats: Arc<ConnectionCacheStats>,
    ) -> Self {
        let inner = Arc::new(NonblockingQuicTpuConnection::new(
            endpoint,
            tpu_addr,
            connection_stats,
        ));
        Self { inner }
    }

    pub fn new_with_client(
        client: Arc<QuicClient>,
        connection_stats: Arc<ConnectionCacheStats>,
    ) -> Self {
        let inner = Arc::new(NonblockingQuicTpuConnection::new_with_client(
            client,
            connection_stats,
        ));
        Self { inner }
    }
}

impl TpuConnection for QuicTpuConnection {
    fn tpu_addr(&self) -> &SocketAddr {
        self.inner.tpu_addr()
    }

    fn send_wire_transaction_batch<T>(&self, buffers: &[T]) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync,
    {
        RUNTIME.block_on(self.inner.send_wire_transaction_batch(buffers))?;
        Ok(())
    }

    fn send_wire_transaction_async(&self, wire_transaction: Vec<u8>) -> TransportResult<()> {
        let inner = self.inner.clone();
        //drop and detach the task
        let _ = RUNTIME.spawn(async move { inner.send_wire_transaction(wire_transaction).await });
        Ok(())
    }

    fn send_wire_transaction_batch_async(&self, buffers: Vec<Vec<u8>>) -> TransportResult<()> {
        let inner = self.inner.clone();
        //drop and detach the task
        let _ = RUNTIME.spawn(async move { inner.send_wire_transaction_batch(&buffers).await });
        Ok(())
    }
}

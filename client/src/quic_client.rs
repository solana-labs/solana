//! Simple client that connects to a given UDP port with the QUIC protocol and provides
//! an interface for sending transactions which is restricted by the server's flow control.

use {
    crate::{
        connection_cache::ConnectionCacheStats,
        nonblocking::quic_client::{QuicClient, QuicLazyInitializedEndpoint},
        tpu_connection::{ClientStats, TpuConnection},
    },
    lazy_static::lazy_static,
    log::*,
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
    client: Arc<QuicClient>,
    connection_stats: Arc<ConnectionCacheStats>,
}

impl QuicTpuConnection {
    pub fn base_stats(&self) -> Arc<ClientStats> {
        self.client.stats()
    }

    pub fn new(
        endpoint: Arc<QuicLazyInitializedEndpoint>,
        tpu_addr: SocketAddr,
        connection_stats: Arc<ConnectionCacheStats>,
    ) -> Self {
        let client = Arc::new(QuicClient::new(endpoint, tpu_addr));

        Self {
            client,
            connection_stats,
        }
    }
}

impl TpuConnection for QuicTpuConnection {
    fn tpu_addr(&self) -> &SocketAddr {
        self.client.tpu_addr()
    }

    fn send_wire_transaction_batch<T>(&self, buffers: &[T]) -> TransportResult<()>
    where
        T: AsRef<[u8]>,
    {
        let stats = ClientStats::default();
        let len = buffers.len();
        let _guard = RUNTIME.enter();
        let send_batch = self
            .client
            .send_batch(buffers, &stats, self.connection_stats.clone());
        let res = RUNTIME.block_on(send_batch);
        self.connection_stats
            .add_client_stats(&stats, len, res.is_ok());
        res?;
        Ok(())
    }

    fn send_wire_transaction_async(&self, wire_transaction: Vec<u8>) -> TransportResult<()> {
        let stats = Arc::new(ClientStats::default());
        let _guard = RUNTIME.enter();
        let client = self.client.clone();
        let connection_stats = self.connection_stats.clone();
        //drop and detach the task
        let _ = RUNTIME.spawn(async move {
            let send_buffer =
                client.send_buffer(wire_transaction, &stats, connection_stats.clone());
            if let Err(e) = send_buffer.await {
                warn!(
                    "Failed to send transaction async to {}, error: {:?} ",
                    client.tpu_addr(),
                    e
                );
                datapoint_warn!("send-wire-async", ("failure", 1, i64),);
                connection_stats.add_client_stats(&stats, 1, false);
            } else {
                connection_stats.add_client_stats(&stats, 1, true);
            }
        });
        Ok(())
    }

    fn send_wire_transaction_batch_async(&self, buffers: Vec<Vec<u8>>) -> TransportResult<()> {
        let stats = Arc::new(ClientStats::default());
        let _guard = RUNTIME.enter();
        let client = self.client.clone();
        let connection_stats = self.connection_stats.clone();
        let len = buffers.len();
        //drop and detach the task
        let _ = RUNTIME.spawn(async move {
            let send_batch = client.send_batch(&buffers, &stats, connection_stats.clone());
            if let Err(e) = send_batch.await {
                warn!("Failed to send transaction batch async to {:?}", e);
                datapoint_warn!("send-wire-batch-async", ("failure", 1, i64),);
                connection_stats.add_client_stats(&stats, len, false);
            } else {
                connection_stats.add_client_stats(&stats, len, true);
            }
        });
        Ok(())
    }
}

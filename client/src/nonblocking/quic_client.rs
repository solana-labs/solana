//! Simple nonblocking client that connects to a given UDP port with the QUIC protocol
//! and provides an interface for sending transactions which is restricted by the
//! server's flow control.
pub use solana_quic_client::nonblocking::quic_client::{
    QuicClient, QuicClientCertificate, QuicError, QuicLazyInitializedEndpoint, QuicTpuConnection,
};
use {
    crate::nonblocking::tpu_connection::TpuConnection,
    async_trait::async_trait,
    log::*,
    solana_sdk::transport::Result as TransportResult,
    solana_tpu_client::tpu_connection::ClientStats,
    std::{net::SocketAddr, sync::Arc},
};

#[async_trait]
impl TpuConnection for QuicTpuConnection {
    fn tpu_addr(&self) -> &SocketAddr {
        self.client.tpu_addr()
    }

    async fn send_wire_transaction_batch<T>(&self, buffers: &[T]) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync,
    {
        let stats = ClientStats::default();
        let len = buffers.len();
        let res = self
            .client
            .send_batch(buffers, &stats, self.connection_stats.clone())
            .await;
        self.connection_stats
            .add_client_stats(&stats, len, res.is_ok());
        res?;
        Ok(())
    }

    async fn send_wire_transaction<T>(&self, wire_transaction: T) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync,
    {
        let stats = Arc::new(ClientStats::default());
        let send_buffer =
            self.client
                .send_buffer(wire_transaction, &stats, self.connection_stats.clone());
        if let Err(e) = send_buffer.await {
            warn!(
                "Failed to send transaction async to {}, error: {:?} ",
                self.tpu_addr(),
                e
            );
            datapoint_warn!("send-wire-async", ("failure", 1, i64),);
            self.connection_stats.add_client_stats(&stats, 1, false);
        } else {
            self.connection_stats.add_client_stats(&stats, 1, true);
        }
        Ok(())
    }
}

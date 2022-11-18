//! Simple UDP client that communicates with the given UDP port with UDP and provides
//! an interface for sending transactions

pub use solana_udp_client::nonblocking::udp_client::UdpTpuConnection;
use {
    crate::nonblocking::tpu_connection::TpuConnection, async_trait::async_trait,
    core::iter::repeat, solana_sdk::transport::Result as TransportResult,
    solana_streamer::nonblocking::sendmmsg::batch_send, std::net::SocketAddr,
};

#[async_trait]
impl TpuConnection for UdpTpuConnection {
    fn tpu_addr(&self) -> &SocketAddr {
        &self.addr
    }

    async fn send_wire_transaction<T>(&self, wire_transaction: T) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync,
    {
        self.socket
            .send_to(wire_transaction.as_ref(), self.addr)
            .await?;
        Ok(())
    }

    async fn send_wire_transaction_batch<T>(&self, buffers: &[T]) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync,
    {
        let pkts: Vec<_> = buffers.iter().zip(repeat(self.tpu_addr())).collect();
        batch_send(&self.socket, &pkts).await?;
        Ok(())
    }
}

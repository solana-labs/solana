//! Simple TPU client that communicates with the given UDP port with UDP and provides
//! an interface for sending transactions

use {
    core::iter::repeat,
    solana_sdk::transport::Result as TransportResult,
    solana_streamer::sendmmsg::batch_send,
    solana_tpu_client::{
        connection_cache_stats::ConnectionCacheStats, tpu_connection::TpuConnection,
    },
    std::{
        net::{SocketAddr, UdpSocket},
        sync::Arc,
    },
};

pub struct UdpTpuConnection {
    pub socket: Arc<UdpSocket>,
    pub addr: SocketAddr,
}

impl UdpTpuConnection {
    pub fn new_from_addr(local_socket: Arc<UdpSocket>, tpu_addr: SocketAddr) -> Self {
        Self {
            socket: local_socket,
            addr: tpu_addr,
        }
    }

    pub fn new(
        local_socket: Arc<UdpSocket>,
        tpu_addr: SocketAddr,
        _connection_stats: Arc<ConnectionCacheStats>,
    ) -> Self {
        Self::new_from_addr(local_socket, tpu_addr)
    }
}

impl TpuConnection for UdpTpuConnection {
    fn tpu_addr(&self) -> &SocketAddr {
        &self.addr
    }

    fn send_wire_transaction_async(&self, wire_transaction: Vec<u8>) -> TransportResult<()> {
        self.socket.send_to(wire_transaction.as_ref(), self.addr)?;
        Ok(())
    }

    fn send_wire_transaction_batch<T>(&self, buffers: &[T]) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync,
    {
        let pkts: Vec<_> = buffers.iter().zip(repeat(self.tpu_addr())).collect();
        batch_send(&self.socket, &pkts)?;
        Ok(())
    }

    fn send_wire_transaction_batch_async(&self, buffers: Vec<Vec<u8>>) -> TransportResult<()> {
        let pkts: Vec<_> = buffers.into_iter().zip(repeat(self.tpu_addr())).collect();
        batch_send(&self.socket, &pkts)?;
        Ok(())
    }
}

//! Simple TPU client that communicates with the given UDP port with UDP and provides
//! an interface for sending transactions

use {
    crate::{connection_cache::ConnectionCacheStats, tpu_connection::TpuConnection},
    core::iter::repeat,
    solana_net_utils::VALIDATOR_PORT_RANGE,
    solana_sdk::transport::Result as TransportResult,
    solana_streamer::sendmmsg::batch_send,
    std::{
        net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
        sync::Arc,
    },
};

pub struct UdpTpuConnection {
    socket: UdpSocket,
    addr: SocketAddr,
}

impl UdpTpuConnection {
    pub fn new(tpu_addr: SocketAddr, _connection_stats: Arc<ConnectionCacheStats>) -> Self {
        let (_, client_socket) = solana_net_utils::bind_in_range(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            VALIDATOR_PORT_RANGE,
        )
        .unwrap();

        Self {
            socket: client_socket,
            addr: tpu_addr,
        }
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
        T: AsRef<[u8]>,
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

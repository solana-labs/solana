//! Simple TPU client that communicates with the given UDP port with UDP and provides
//! an interface for sending transactions

use {
    crate::{nonblocking::tpu_connection::TpuConnection, tpu_connection::ClientStats},
    async_trait::async_trait,
    core::iter::repeat,
    solana_net_utils::VALIDATOR_PORT_RANGE,
    solana_sdk::transport::Result as TransportResult,
    solana_streamer::nonblocking::sendmmsg::batch_send,
    std::net::{IpAddr, Ipv4Addr, SocketAddr},
    tokio::net::UdpSocket,
};

pub struct UdpTpuConnection {
    socket: UdpSocket,
    addr: SocketAddr,
}

#[async_trait]
impl TpuConnection for UdpTpuConnection {
    fn new(tpu_addr: SocketAddr) -> Self {
        let (_, client_socket) = solana_net_utils::bind_in_range(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            VALIDATOR_PORT_RANGE,
        )
        .unwrap();
        client_socket.set_nonblocking(true).unwrap();
        let socket = UdpSocket::from_std(client_socket).unwrap();
        Self {
            socket,
            addr: tpu_addr,
        }
    }

    fn tpu_addr(&self) -> &SocketAddr {
        &self.addr
    }

    async fn send_wire_transaction<T>(
        &self,
        wire_transaction: T,
        _stats: &ClientStats,
    ) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync,
    {
        self.socket
            .send_to(wire_transaction.as_ref(), self.addr)
            .await?;
        Ok(())
    }

    async fn send_wire_transaction_batch<T>(
        &self,
        buffers: &[T],
        _stats: &ClientStats,
    ) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync,
    {
        let pkts: Vec<_> = buffers.iter().zip(repeat(self.tpu_addr())).collect();
        batch_send(&self.socket, &pkts).await?;
        Ok(())
    }
}

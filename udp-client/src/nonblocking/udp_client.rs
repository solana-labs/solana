//! Simple UDP client that communicates with the given UDP port with UDP and provides
//! an interface for sending transactions

use {
    async_trait::async_trait, core::iter::repeat, solana_sdk::transport::Result as TransportResult,
    solana_streamer::nonblocking::sendmmsg::batch_send,
    solana_tpu_client::nonblocking::tpu_connection::TpuConnection, std::net::SocketAddr,
    tokio::net::UdpSocket,
};

pub struct UdpTpuConnection {
    pub socket: UdpSocket,
    pub addr: SocketAddr,
}

impl UdpTpuConnection {
    pub fn new_from_addr(socket: std::net::UdpSocket, tpu_addr: SocketAddr) -> Self {
        socket.set_nonblocking(true).unwrap();
        let socket = UdpSocket::from_std(socket).unwrap();
        Self {
            socket,
            addr: tpu_addr,
        }
    }
}

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

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::packet::{Packet, PACKET_DATA_SIZE},
        solana_streamer::nonblocking::recvmmsg::recv_mmsg,
        std::net::{IpAddr, Ipv4Addr},
        tokio::net::UdpSocket,
    };

    async fn check_send_one(connection: &UdpTpuConnection, reader: &UdpSocket) {
        let packet = vec![111u8; PACKET_DATA_SIZE];
        connection.send_wire_transaction(&packet).await.unwrap();
        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(reader, &mut packets[..]).await.unwrap();
        assert_eq!(1, recv);
    }

    async fn check_send_batch(connection: &UdpTpuConnection, reader: &UdpSocket) {
        let packets: Vec<_> = (0..32).map(|_| vec![0u8; PACKET_DATA_SIZE]).collect();
        connection
            .send_wire_transaction_batch(&packets)
            .await
            .unwrap();
        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(reader, &mut packets[..]).await.unwrap();
        assert_eq!(32, recv);
    }

    #[tokio::test]
    async fn test_send_from_addr() {
        let addr_str = "0.0.0.0:50100";
        let addr = addr_str.parse().unwrap();
        let socket =
            solana_net_utils::bind_with_any_port(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))).unwrap();
        let connection = UdpTpuConnection::new_from_addr(socket, addr);
        let reader = UdpSocket::bind(addr_str).await.expect("bind");
        check_send_one(&connection, &reader).await;
        check_send_batch(&connection, &reader).await;
    }
}

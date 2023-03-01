//! Simple UDP client that communicates with the given UDP port with UDP and provides
//! an interface for sending data

use {
    async_trait::async_trait, core::iter::repeat,
    solana_connection_cache::nonblocking::client_connection::ClientConnection,
    solana_sdk::transport::Result as TransportResult,
    solana_streamer::nonblocking::sendmmsg::batch_send, std::net::SocketAddr,
    tokio::net::UdpSocket,
};

pub struct UdpClientConnection {
    pub socket: UdpSocket,
    pub addr: SocketAddr,
}

impl UdpClientConnection {
    pub fn new_from_addr(socket: std::net::UdpSocket, server_addr: SocketAddr) -> Self {
        socket.set_nonblocking(true).unwrap();
        let socket = UdpSocket::from_std(socket).unwrap();
        Self {
            socket,
            addr: server_addr,
        }
    }
}

#[async_trait]
impl ClientConnection for UdpClientConnection {
    fn server_addr(&self) -> &SocketAddr {
        &self.addr
    }

    async fn send_data(&self, buffer: &[u8]) -> TransportResult<()> {
        self.socket.send_to(buffer, self.addr).await?;
        Ok(())
    }

    async fn send_data_batch(&self, buffers: &[Vec<u8>]) -> TransportResult<()> {
        let pkts: Vec<_> = buffers.iter().zip(repeat(self.server_addr())).collect();
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

    async fn check_send_one(connection: &UdpClientConnection, reader: &UdpSocket) {
        let packet = vec![111u8; PACKET_DATA_SIZE];
        connection.send_data(&packet).await.unwrap();
        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(reader, &mut packets[..]).await.unwrap();
        assert_eq!(1, recv);
    }

    async fn check_send_batch(connection: &UdpClientConnection, reader: &UdpSocket) {
        let packets: Vec<_> = (0..32).map(|_| vec![0u8; PACKET_DATA_SIZE]).collect();
        connection.send_data_batch(&packets).await.unwrap();
        let mut packets = vec![Packet::default(); 32];
        let recv = recv_mmsg(reader, &mut packets[..]).await.unwrap();
        assert_eq!(32, recv);
    }

    #[tokio::test]
    async fn test_send_from_addr() {
        let addr_str = "0.0.0.0:50100";
        let addr = addr_str.parse().unwrap();
        let socket =
            solana_net_utils::bind_with_any_port(IpAddr::V4(Ipv4Addr::UNSPECIFIED)).unwrap();
        let connection = UdpClientConnection::new_from_addr(socket, addr);
        let reader = UdpSocket::bind(addr_str).await.expect("bind");
        check_send_one(&connection, &reader).await;
        check_send_batch(&connection, &reader).await;
    }
}

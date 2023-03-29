//! Simple client that communicates with the given UDP port with UDP and provides
//! an interface for sending data

use {
    core::iter::repeat,
    solana_connection_cache::client_connection::ClientConnection,
    solana_sdk::transport::Result as TransportResult,
    solana_streamer::sendmmsg::batch_send,
    std::{
        net::{SocketAddr, UdpSocket},
        sync::Arc,
    },
};

pub struct UdpClientConnection {
    pub socket: Arc<UdpSocket>,
    pub addr: SocketAddr,
}

impl UdpClientConnection {
    pub fn new_from_addr(local_socket: Arc<UdpSocket>, server_addr: SocketAddr) -> Self {
        Self {
            socket: local_socket,
            addr: server_addr,
        }
    }
}

impl ClientConnection for UdpClientConnection {
    fn server_addr(&self) -> &SocketAddr {
        &self.addr
    }

    fn send_data_async(&self, data: Vec<u8>) -> TransportResult<()> {
        self.socket.send_to(data.as_ref(), self.addr)?;
        Ok(())
    }

    fn send_data_batch(&self, buffers: &[Vec<u8>]) -> TransportResult<()> {
        let pkts: Vec<_> = buffers.iter().zip(repeat(self.server_addr())).collect();
        batch_send(&self.socket, &pkts)?;
        Ok(())
    }

    fn send_data_batch_async(&self, buffers: Vec<Vec<u8>>) -> TransportResult<()> {
        let pkts: Vec<_> = buffers
            .into_iter()
            .zip(repeat(self.server_addr()))
            .collect();
        batch_send(&self.socket, &pkts)?;
        Ok(())
    }

    fn send_data(&self, buffer: &[u8]) -> TransportResult<()> {
        self.socket.send_to(buffer, self.addr)?;
        Ok(())
    }
}

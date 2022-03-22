//! Simple TPU client that communicates with the given UDP port with UDP and provides
//! an interface for sending transactions

use {
    crate::tpu_connection::TpuConnection,
    solana_sdk::transport::Result as TransportResult,
    std::net::{SocketAddr, UdpSocket},
};

pub struct UdpTpuConnection {
    socket: UdpSocket,
    addr: SocketAddr,
}

impl TpuConnection for UdpTpuConnection {
    fn new(client_socket: UdpSocket, tpu_addr: SocketAddr) -> Self {
        Self {
            socket: client_socket,
            addr: tpu_addr,
        }
    }

    fn tpu_addr(&self) -> &SocketAddr {
        &self.addr
    }

    fn send_wire_transaction(&self, wire_transaction: &[u8]) -> TransportResult<()> {
        self.socket.send_to(wire_transaction, self.addr)?;
        Ok(())
    }
}

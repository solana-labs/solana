//! Simple TPU client that communicates with the given UDP port with UDP and provides
//! an interface for sending transactions

use {
    crate::tpu_connection::TpuConnection,
    solana_sdk::{transaction::Transaction, transport::Result as TransportResult},
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

    fn send_wire_transaction(&self, data: &[u8]) -> TransportResult<()> {
        self.socket.send_to(data, self.addr)?;
        Ok(())
    }

    fn send_batch(&self, transactions: &[Transaction]) -> TransportResult<()> {
        transactions
            .iter()
            .map(|tx| bincode::serialize(&tx).expect("serialize Transaction in send_batch"))
            .try_for_each(|buff| -> TransportResult<()> {
                self.socket.send_to(&buff, self.addr)?;
                Ok(())
            })?;
        Ok(())
    }
}

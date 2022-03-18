use {
    solana_sdk::transport::Result as TransportResult,
    std::net::{SocketAddr, UdpSocket},
};

pub trait TpuConnection {
    fn new(client_socket: UdpSocket, tpu_addr: SocketAddr) -> Self
    where
        Self: Sized;

    fn tpu_addr(&self) -> &SocketAddr;

    fn send_wire_transaction(&self, wire_transaction: &[u8]) -> TransportResult<()>;

    fn send_wire_transaction_batch(
        &self,
        wire_transaction_batch: &[Vec<u8>],
    ) -> TransportResult<()> {
        for wire_transaction in wire_transaction_batch {
            self.send_wire_transaction(wire_transaction)?;
        }
        Ok(())
    }
}

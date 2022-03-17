use {
    solana_sdk::{transaction::Transaction, transport::Result as TransportResult},
    std::net::{SocketAddr, UdpSocket},
};

pub trait TpuConnection {
    fn new(client_socket: UdpSocket, tpu_addr: SocketAddr) -> Self
    where
        Self: Sized;

    fn tpu_addr(&self) -> &SocketAddr;

    fn send_transaction(&self, tx: &Transaction) -> TransportResult<()> {
        let data = bincode::serialize(tx).expect("serialize Transaction in send_transaction");
        self.send_wire_transaction(&data)
    }

    fn send_wire_transaction(&self, data: &[u8]) -> TransportResult<()>;

    fn send_batch(&self, transactions: &[Transaction]) -> TransportResult<()>;
}

use {
    rayon::iter::{IntoParallelRefIterator, ParallelIterator},
    solana_sdk::{transaction::VersionedTransaction, transport::Result as TransportResult},
    std::net::{SocketAddr, UdpSocket},
};

pub trait TpuConnection {
    fn new(client_socket: UdpSocket, tpu_addr: SocketAddr) -> Self
    where
        Self: Sized;

    fn tpu_addr(&self) -> &SocketAddr;

    fn serialize_and_send_transaction(
        &self,
        transaction: &VersionedTransaction,
    ) -> TransportResult<()> {
        let wire_transaction =
            bincode::serialize(transaction).expect("serialize Transaction in send_batch");
        self.send_wire_transaction(&wire_transaction)
    }

    fn send_wire_transaction(&self, wire_transaction: &[u8]) -> TransportResult<()>;

    fn par_serialize_and_send_transaction_batch(
        &self,
        transaction_batch: &[VersionedTransaction],
    ) -> TransportResult<()> {
        let wire_transaction_batch: Vec<_> = transaction_batch
            .par_iter()
            .map(|tx| bincode::serialize(&tx).expect("serialize Transaction in send_batch"))
            .collect();
        self.send_wire_transaction_batch(&wire_transaction_batch)
    }

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

pub use solana_tpu_client::tpu_connection::ClientStats;
use {
    enum_dispatch::enum_dispatch,
    rayon::iter::{IntoParallelIterator, ParallelIterator},
    solana_quic_client::quic_client::QuicTpuConnection,
    solana_sdk::{transaction::VersionedTransaction, transport::Result as TransportResult},
    solana_udp_client::udp_client::UdpTpuConnection,
    std::net::SocketAddr,
};

#[enum_dispatch]
pub enum BlockingConnection {
    UdpTpuConnection,
    QuicTpuConnection,
}

#[enum_dispatch(BlockingConnection)]
pub trait TpuConnection {
    fn tpu_addr(&self) -> &SocketAddr;

    fn serialize_and_send_transaction(
        &self,
        transaction: &VersionedTransaction,
    ) -> TransportResult<()> {
        let wire_transaction =
            bincode::serialize(transaction).expect("serialize Transaction in send_batch");
        self.send_wire_transaction(wire_transaction)
    }

    fn send_wire_transaction<T>(&self, wire_transaction: T) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync,
    {
        self.send_wire_transaction_batch(&[wire_transaction])
    }

    fn send_wire_transaction_async(&self, wire_transaction: Vec<u8>) -> TransportResult<()>;

    fn par_serialize_and_send_transaction_batch(
        &self,
        transactions: &[VersionedTransaction],
    ) -> TransportResult<()> {
        let buffers = transactions
            .into_par_iter()
            .map(|tx| bincode::serialize(&tx).expect("serialize Transaction in send_batch"))
            .collect::<Vec<_>>();

        self.send_wire_transaction_batch(&buffers)
    }

    fn send_wire_transaction_batch<T>(&self, buffers: &[T]) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync;

    fn send_wire_transaction_batch_async(&self, buffers: Vec<Vec<u8>>) -> TransportResult<()>;
}

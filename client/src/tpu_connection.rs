use {
    rayon::iter::{IntoParallelIterator, ParallelIterator},
    solana_sdk::{transaction::VersionedTransaction, transport::Result as TransportResult},
    std::{
        net::{SocketAddr, UdpSocket},
        sync::{Arc, atomic::AtomicU64},
    },
};

#[derive(Default)]
pub struct ClientStats {
    pub total_connections: AtomicU64,
    pub connection_reuse: AtomicU64,
    pub connection_errors: AtomicU64,
}

pub trait TpuConnection {
    fn new(client_socket: UdpSocket, tpu_addr: SocketAddr) -> Self;

    fn tpu_addr(&self) -> &SocketAddr;

    fn serialize_and_send_transaction(
        &self,
        transaction: &VersionedTransaction,
        stats: &ClientStats,
    ) -> TransportResult<()> {
        let wire_transaction =
            bincode::serialize(transaction).expect("serialize Transaction in send_batch");
        self.send_wire_transaction(&wire_transaction, stats)
    }

    fn send_wire_transaction<T>(
        &self,
        wire_transaction: T,
        stats: &ClientStats,
    ) -> TransportResult<()>
    where
        T: AsRef<[u8]>;

    fn send_wire_transaction_async(&self, wire_transaction: Vec<u8>, stats: Arc<ClientStats>) -> TransportResult<()>;

    fn par_serialize_and_send_transaction_batch(
        &self,
        transactions: &[VersionedTransaction],
        stats: &ClientStats,
    ) -> TransportResult<()> {
        let buffers = transactions
            .into_par_iter()
            .map(|tx| bincode::serialize(&tx).expect("serialize Transaction in send_batch"))
            .collect::<Vec<_>>();

        self.send_wire_transaction_batch(&buffers, stats)
    }

    fn send_wire_transaction_batch<T>(
        &self,
        buffers: &[T],
        stats: &ClientStats,
    ) -> TransportResult<()>
    where
        T: AsRef<[u8]>;
}

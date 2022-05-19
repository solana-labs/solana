use {
    crate::tpu_connection::ClientStats,
    async_trait::async_trait,
    rayon::iter::{IntoParallelIterator, ParallelIterator},
    solana_sdk::{transaction::VersionedTransaction, transport::Result as TransportResult},
    std::net::SocketAddr,
};

#[async_trait]
pub trait TpuConnection {
    fn new(tpu_addr: SocketAddr) -> Self;

    fn tpu_addr(&self) -> &SocketAddr;

    async fn serialize_and_send_transaction(
        &self,
        transaction: &VersionedTransaction,
        stats: &ClientStats,
    ) -> TransportResult<()> {
        let wire_transaction =
            bincode::serialize(transaction).expect("serialize Transaction in send_batch");
        self.send_wire_transaction(&wire_transaction, stats).await
    }

    async fn send_wire_transaction<T>(
        &self,
        wire_transaction: T,
        stats: &ClientStats,
    ) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync;

    async fn par_serialize_and_send_transaction_batch(
        &self,
        transactions: &[VersionedTransaction],
        stats: &ClientStats,
    ) -> TransportResult<()> {
        let buffers = transactions
            .into_par_iter()
            .map(|tx| bincode::serialize(&tx).expect("serialize Transaction in send_batch"))
            .collect::<Vec<_>>();

        self.send_wire_transaction_batch(&buffers, stats).await
    }

    async fn send_wire_transaction_batch<T>(
        &self,
        buffers: &[T],
        stats: &ClientStats,
    ) -> TransportResult<()>
    where
        T: AsRef<[u8]> + Send + Sync;
}
